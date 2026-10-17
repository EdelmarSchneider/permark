use super::*;

const NO_CHANGES: [Change; 0] = [];

/// The changes between two texts.
fn diff(reviewed: &str, current: &str) -> Vec<Change> {
    Comparison::new(reviewed, current).changes
}

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
    Side::new(text)
        .keys
        .iter()
        .map(ToString::to_string)
        .collect()
}

/// Reverts one change after another (the first each time, the diff taken again) until nothing is left. `Err` when a
/// change cannot be reverted on its own (the person is then offered Revert all).
fn revert_everything(reviewed: &str, current: &str) -> Result<String, CannotSplice> {
    let mut now = current.to_owned();
    for _ in 0..200 {
        let next = {
            let cmp = Comparison::new(reviewed, &now);
            if cmp.changes().is_empty() {
                return Ok(now);
            }
            cmp.reverted(0)?
        };
        now = next;
    }
    panic!("reverting does not end: {now:?}");
}

/// Keeps one change after another until nothing is left (see [`revert_everything`]).
fn keep_everything(reviewed: &str, current: &str) -> Result<String, CannotSplice> {
    let mut now = reviewed.to_owned();
    for _ in 0..200 {
        let next = {
            let cmp = Comparison::new(&now, current);
            if cmp.changes().is_empty() {
                return Ok(now);
            }
            cmp.kept(0)?
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
    assert_eq!(keep_everything(reviewed, current).expect("kept"), current);
    assert_eq!(
        block_texts(&revert_everything(reviewed, current).expect("reverted")),
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
    assert_eq!(
        cmp.reverted(0).expect("reverted"),
        reviewed,
        "the whole file, as it was"
    );
    assert_eq!(cmp.kept(0).expect("kept"), current, "and as it is");
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
    assert_eq!(cmp.reverted(0).expect("reverted"), reviewed);
    assert_eq!(cmp.kept(0).expect("kept"), current);
}

#[test]
fn reverting_a_removed_block_puts_it_back_with_its_separator() {
    let reviewed = "# A\n\nfirst\n\ngone\n\nlast\n";
    let current = "# A\n\nfirst\n\nlast\n";
    let cmp = Comparison::new(reviewed, current);
    assert_eq!(cmp.reverted(0).expect("reverted"), reviewed);
    assert_eq!(cmp.kept(0).expect("kept"), current);
}

#[test]
fn an_addition_at_the_start_or_at_the_end_keeps_the_final_newline_in_place() {
    let reviewed = "middle\n";
    let at_start = "first\n\nmiddle\n";
    let at_end = "middle\n\nlast\n";
    for current in [at_start, at_end] {
        let cmp = Comparison::new(reviewed, current);
        assert_eq!(cmp.reverted(0).expect("reverted"), reviewed, "{current:?}");
        assert_eq!(cmp.kept(0).expect("kept"), current, "{current:?}");
    }
    // and the same for removals
    for reviewed in [at_start, at_end] {
        let cmp = Comparison::new(reviewed, "middle\n");
        assert_eq!(cmp.reverted(0).expect("reverted"), reviewed, "{reviewed:?}");
        assert_eq!(cmp.kept(0).expect("kept"), "middle\n", "{reviewed:?}");
    }
}

#[test]
fn a_reverted_edit_swaps_only_the_blocks() {
    let reviewed = "# A\n\n\nThe quick brown fox jumps.\n\n\nend\n";
    let current = "# A\n\nThe quick brown fox leaps over.\n\nend\n";
    let cmp = Comparison::new(reviewed, current);
    assert_eq!(
        cmp.reverted(0).expect("reverted"),
        "# A\n\nThe quick brown fox jumps.\n\nend\n",
        "the file's own blank lines stay"
    );
    assert_eq!(
        cmp.kept(0).expect("kept"),
        "# A\n\n\nThe quick brown fox leaps over.\n\n\nend\n"
    );
}

#[test]
fn taking_out_a_block_that_follows_a_paragraph_straight_on_does_not_glue_the_paragraphs() {
    let with_list = "para one\n- list item\n\npara two\n";
    let without = "para one\n\npara two\n";
    // the list is a block of its own, tight against the paragraph
    assert_eq!(
        block_texts(with_list),
        ["para one", "- list item", "para two"]
    );
    let cmp = Comparison::new(with_list, without);
    assert_eq!(cmp.changes().len(), 1);
    assert_eq!(cmp.kept(0).expect("kept"), without, "accepting the removal");
    assert_eq!(
        cmp.reverted(0).expect("reverted"),
        with_list,
        "and putting the list back, exactly"
    );
    // and the other way round
    let cmp = Comparison::new(without, with_list);
    assert_eq!(cmp.reverted(0).expect("reverted"), without);
    assert_eq!(cmp.kept(0).expect("kept"), with_list);
    assert_eq!(
        block_texts(&cmp.reverted(0).expect("reverted")),
        ["para one", "para two"]
    );
}

#[test]
fn an_edited_block_that_follows_a_paragraph_straight_on_stays_a_block_of_its_own() {
    let list = "para one\n- list item\n\nend\n";
    let plain = "para one\n\nPlain words here\n\nend\n";
    let cmp = Comparison::new(list, plain);
    assert_eq!(cmp.changes().len(), 1);
    assert_eq!(
        block_texts(&cmp.kept(0).expect("kept")),
        ["para one", "Plain words here", "end"]
    );
    assert_eq!(cmp.kept(0).expect("kept"), plain);
    // the file being changed has a blank line there, and keeps it
    assert_eq!(
        cmp.reverted(0).expect("reverted"),
        "para one\n\n- list item\n\nend\n"
    );
    // a heading may follow a paragraph straight on, and a list may take its place
    let heading = "para one\n# Heading now\n\nend\n";
    let cmp = Comparison::new(heading, list);
    assert_eq!(cmp.kept(0).expect("kept"), list);
    assert_eq!(cmp.reverted(0).expect("reverted"), heading);
}

#[test]
fn a_reverted_block_takes_the_line_endings_of_the_file_it_goes_into() {
    let reviewed = "# A\n\nfirst\nsecond\n\nlast\n";
    let current = "# A\r\n\r\nlast\r\n";
    let cmp = Comparison::new(reviewed, current);
    assert_eq!(cmp.changes().len(), 1);
    assert_eq!(
        cmp.reverted(0).expect("reverted"),
        "# A\r\n\r\nfirst\r\nsecond\r\n\r\nlast\r\n"
    );
}

#[test]
fn keeping_one_change_leaves_the_others_to_review() {
    let reviewed = "# T\n\nalpha one\n\nstable\n\nbeta two\n\n# End\n";
    let current = "# T\n\nalpha ONE and more\n\nstable\n\nbeta TWO also\n\n# End\n";
    let cmp = Comparison::new(reviewed, current);
    let after_keep = cmp.kept(0).expect("kept");
    assert_eq!(
        after_keep,
        "# T\n\nalpha ONE and more\n\nstable\n\nbeta two\n\n# End\n"
    );
    let again = diff(&after_keep, current);
    assert_eq!(again.len(), 1, "the other edit is still to review");
    let after_revert = cmp.reverted(1).expect("reverted");
    assert_eq!(
        after_revert,
        "# T\n\nalpha ONE and more\n\nstable\n\nbeta two\n\n# End\n"
    );
}

// ---- text between blocks ----

#[test]
fn a_link_reference_definition_is_a_block_of_its_own() {
    // comrak makes no node of it, so the editor's blocks leave it in the separator
    let text = "See [docs][d].\n\n[d]: https://good.example\n\n# End\n";
    assert_eq!(parse_blocks(text).blocks.len(), 2);
    let side = Side::new(text);
    assert_eq!(side.blocks.len(), 3);
    assert_eq!(side.block_text(1), "[d]: https://good.example");
    assert_eq!(side.blocks[1].kind, BlockKind::Other);
}

#[test]
fn a_run_of_definitions_is_one_block_and_blank_lines_split_runs() {
    let text = "[a]: https://a.example\n[b]: https://b.example\n\n[c]: https://c.example\n";
    let side = Side::new(text);
    assert_eq!(
        (0..side.blocks.len())
            .map(|i| side.block_text(i))
            .collect::<Vec<_>>(),
        [
            "[a]: https://a.example\n[b]: https://b.example",
            "[c]: https://c.example"
        ]
    );
}

#[test]
fn changing_where_a_link_points_is_a_change_to_review() {
    let reviewed = "See [docs][d].\n\n[d]: https://good.example\n";
    let current = "See [docs][d].\n\n[d]: https://evil.example\n";
    assert_eq!(
        shape(reviewed, current),
        [(
            ChangeKind::Modified,
            "[d]: https://good.example".to_owned(),
            "[d]: https://evil.example".to_owned()
        )]
    );
    let cmp = Comparison::new(reviewed, current);
    assert_eq!(cmp.reverted(0).expect("reverted"), reviewed);
    assert_eq!(cmp.kept(0).expect("kept"), current);
}

#[test]
fn a_definition_that_is_added_or_removed_is_shown_not_moved_along_unseen() {
    let reviewed = "# T\n\nPara.\n";
    let current = "# T\n\nNew para.\n\n[d]: https://x.example\n\nPara.\n";
    let cmp = Comparison::new(reviewed, current);
    // the new paragraph and the definition are shown together as what was added
    assert_eq!(cmp.changes().len(), 1);
    assert_eq!(cmp.changes()[0].kind, ChangeKind::Added);
    assert_eq!(cmp.changes()[0].new_blocks, 1..3);
    assert_eq!(cmp.reverted(0).expect("reverted"), reviewed);
    // and only the definition, when that is all there is
    let only = "# T\n\nPara.\n\n[d]: https://x.example\n";
    let cmp = Comparison::new(reviewed, only);
    assert_eq!(
        shape(reviewed, only),
        [(
            ChangeKind::Added,
            String::new(),
            "[d]: https://x.example".to_owned()
        )]
    );
    assert_eq!(cmp.reverted(0).expect("reverted"), reviewed);
    assert_eq!(cmp.kept(0).expect("kept"), only);
}

#[test]
fn a_text_too_large_to_parse_has_no_blocks_to_compare() {
    assert_eq!(parse_blocks(&"x\n\n".repeat(3_000_000)).blocks.len(), 0);
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

const PIECES: [&str; 14] = [
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
    "[r]: https://example.com/a",
    "[r]: https://example.com/b \"Title\"",
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
    // mostly blank lines, sometimes the next block straight on (which makes it part of the one before, or not,
    // depending on what the two are)
    let mut text = String::new();
    for (i, block) in blocks.iter().enumerate() {
        if i > 0 {
            text.push_str(["\n\n", "\n\n", "\n\n\n", "\n \n", "\n"][rng.next(5)]);
        }
        text.push_str(block);
    }
    if rng.next(2) == 0 {
        text.push('\n');
    }
    text.replace('\n', eol)
}

#[test]
fn reverting_everything_gives_back_the_reviewed_blocks_and_keeping_everything_the_current_ones() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut refused = 0;
    for round in 0..400 {
        let eol = if round % 5 == 0 { "\r\n" } else { "\n" };
        let reviewed = random_doc(&mut rng, eol);
        let current = random_doc(&mut rng, if round % 7 == 0 { "\r\n" } else { "\n" });
        // a change that cannot be applied on its own is refused (the person is offered the whole at once)
        match revert_everything(&reviewed, &current) {
            Ok(text) => assert_eq!(
                block_texts(&text),
                block_texts(&reviewed),
                "revert, round {round}: {reviewed:?} -> {current:?}"
            ),
            Err(CannotSplice) => refused += 1,
        }
        match keep_everything(&reviewed, &current) {
            Ok(text) => assert_eq!(
                block_texts(&text),
                block_texts(&current),
                "keep, round {round}: {reviewed:?} -> {current:?}"
            ),
            Err(CannotSplice) => refused += 1,
        }
    }
    assert!(refused <= 12, "{refused} of 800 are refused");
}

/// The blocks a document must have once change `index` is swapped: `from`'s blocks, with the blocks of `to`
/// where the change is. The same for keeping and reverting, seen from the side that is being changed.
fn blocks_after(
    from: &[String],
    replace: &Range<usize>,
    to: &[String],
    with: &Range<usize>,
) -> Vec<String> {
    let mut wanted = from[..replace.start].to_vec();
    wanted.extend_from_slice(&to[with.clone()]);
    wanted.extend_from_slice(&from[replace.end..]);
    wanted
}

#[test]
fn every_single_keep_and_revert_gives_exactly_the_blocks_the_review_showed() {
    let mut rng = Rng(0x2545_F491_4F6C_DD1D);
    let (mut total, mut refused) = (0_u32, 0_u32);
    for round in 0..3000 {
        let eol = if round % 5 == 0 { "\r\n" } else { "\n" };
        let reviewed = random_doc(&mut rng, eol);
        let current = random_doc(&mut rng, if round % 7 == 0 { "\r\n" } else { "\n" });
        let (old_blocks, new_blocks) = (block_texts(&reviewed), block_texts(&current));
        let cmp = Comparison::new(&reviewed, &current);
        for (i, change) in cmp.changes().iter().enumerate() {
            total += 2;
            match cmp.reverted(i) {
                Ok(text) => assert_eq!(
                    block_texts(&text),
                    blocks_after(
                        &new_blocks,
                        &change.new_blocks,
                        &old_blocks,
                        &change.old_blocks
                    ),
                    "revert {i}, round {round}: {reviewed:?} -> {current:?}"
                ),
                Err(CannotSplice) => refused += 1,
            }
            match cmp.kept(i) {
                Ok(text) => assert_eq!(
                    block_texts(&text),
                    blocks_after(
                        &old_blocks,
                        &change.old_blocks,
                        &new_blocks,
                        &change.new_blocks
                    ),
                    "keep {i}, round {round}: {reviewed:?} -> {current:?}"
                ),
                Err(CannotSplice) => refused += 1,
            }
        }
    }
    assert!(total > 5000, "{total} splices tried");
    // what cannot be applied alone is refused, and that is rare: a line of dashes that turns the top of the file
    // into front matter is the kind of thing that does it
    assert!(refused * 200 <= total, "{refused} of {total} are refused");
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

// ---- a splice must give the blocks the review showed ----

#[test]
fn a_change_that_would_turn_the_top_of_the_file_into_front_matter_cannot_be_applied_alone() {
    // Putting the dash line back at the top, with another dash line further down, is front matter.
    let reviewed = "---\nTiny.\n\n\n[r]: https://example.com/b \"Title\"\n";
    let current = "Tiny.\n\n\n---";
    let cmp = Comparison::new(reviewed, current);
    assert_eq!(cmp.changes().len(), 2);
    assert_eq!(cmp.reverted(0), Err(CannotSplice));
    assert_eq!(cmp.kept(1), Err(CannotSplice));
    // the other halves are fine, and so is everything at once
    assert_eq!(
        cmp.reverted(1).expect("reverted"),
        "Tiny.\n\n\n[r]: https://example.com/b \"Title\""
    );
    assert_eq!(
        cmp.kept(0).expect("kept"),
        "Tiny.\n\n\n[r]: https://example.com/b \"Title\"\n"
    );
}

#[test]
fn blocks_that_would_run_together_are_kept_apart_by_a_blank_line() {
    // As the reviewed text has them, the definition and the table are tight against each other; put back after
    // other text, that glues them (the definition then shows up as a paragraph).
    let reviewed = "[r]: https://example.com/b \"Title\"\n| a | b |\n|---|---|\n| 1 | 2 |\n";
    let current = "1. first\n2. second\n\n\nClosing remarks about the whole thing.\n\n\n[r]: https://example.com/b \"Title\"\n";
    let cmp = Comparison::new(reviewed, current);
    assert_eq!(cmp.changes().len(), 2);
    let text = cmp.reverted(1).expect("reverted");
    assert_eq!(
        text,
        "1. first\n2. second\n\n\nClosing remarks about the whole thing.\n\n\n[r]: https://example.com/b \"Title\"\n\n| a | b |\n|---|---|\n| 1 | 2 |\n"
    );
    assert_eq!(
        block_texts(&text),
        [
            "1. first\n2. second",
            "Closing remarks about the whole thing.",
            "[r]: https://example.com/b \"Title\"",
            "| a | b |\n|---|---|\n| 1 | 2 |"
        ]
    );
}

#[test]
fn a_blank_line_between_swapped_blocks_takes_the_line_ending_of_the_file() {
    let source = Side::new(
        "para one
- list item

para two
",
    );
    let target = Side::new(
        "para one

para two
",
    );
    assert_eq!(
        splice_spaced(&target, &(1..1), &source, &(1..2)),
        "para one

- list item

para two
"
    );
    // and nothing is added where nothing goes in, or where there is no neighbour
    assert_eq!(
        splice_spaced(&source, &(1..2), &target, &(1..1)),
        "para one

para two
"
    );
    assert_eq!(
        splice_spaced(&source, &(2..3), &target, &(2..2)),
        "para one
- list item
"
    );
}

// ---- lines ----

#[test]
fn blocks_know_their_lines_as_the_reading_view_numbers_them() {
    let text = "---\ntitle: x\n---\n\n# T\n\nfirst\nsecond\n\n[d]: https://x.example\n\n- a\n- b\n";
    let cmp = Comparison::new("", text);
    let lines: Vec<(usize, usize)> = (0..5).map(|i| cmp.new_lines(i)).collect();
    assert_eq!(lines, [(1, 3), (5, 5), (7, 8), (10, 10), (12, 13)]);
}

#[test]
fn lines_count_every_kind_of_line_break_and_skip_a_byte_order_mark() {
    let crlf = "\u{FEFF}# T\r\n\r\nbody\r\nmore\r\n";
    let cmp = Comparison::new("", crlf);
    assert_eq!((cmp.new_lines(0), cmp.new_lines(1)), ((1, 1), (3, 4)));
    let cr = "# T\r\rbody\rmore";
    let cmp = Comparison::new("", cr);
    assert_eq!((cmp.new_lines(0), cmp.new_lines(1)), ((1, 1), (3, 4)));
    // no break at the end of the last line
    let cmp = Comparison::new(crlf, "");
    assert_eq!(cmp.old_lines(1), (3, 4));
}

#[test]
fn a_change_is_placed_at_the_line_of_its_new_blocks_or_the_block_it_sits_in_front_of() {
    let reviewed = "# T\n\ngone\n\nkeep\n\nold tail\n";
    let current = "# T\n\nkeep\n\nnew tail\n\nmore\n";
    let cmp = Comparison::new(reviewed, current);
    let at: Vec<(ChangeKind, Option<usize>)> = cmp
        .changes()
        .iter()
        .map(|c| (c.kind, cmp.new_line(c)))
        .collect();
    assert_eq!(
        at,
        [
            (ChangeKind::Removed, Some(3)),
            (ChangeKind::Modified, Some(5))
        ]
    );
    let cmp = Comparison::new("a\n\nb\n", "a\n");
    assert_eq!(
        cmp.new_line(&cmp.changes()[0]),
        None,
        "a removal at the end"
    );
}
