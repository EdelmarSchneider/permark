use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;

use super::*;
use crate::vault::Worktree;
use crate::watch::{ChangeKind, FileSig, FsChange, SigStore};

/// Records the renames the service reports to what follows them.
#[derive(Default)]
struct Recorder {
    renamed: Mutex<Vec<(String, String, bool)>>,
}

impl Followers for Recorder {
    fn renamed(&self, from: &str, to: &str, is_dir: bool) {
        lock(&self.renamed).push((from.to_owned(), to.to_owned(), is_dir));
    }
}

/// A recycle bin that is a folder of its own, so a test never touches the real one.
struct FolderBin {
    bin: TempDir,
    fail: bool,
}

impl Recycler for FolderBin {
    fn recycle(&self, paths: &[PathBuf]) -> Result<(), String> {
        if self.fail {
            return Err("the bin is full".to_owned());
        }
        for (i, path) in paths.iter().enumerate() {
            let name = path.file_name().expect("name").to_string_lossy();
            fs::rename(path, self.bin.path().join(format!("{i}-{name}")))
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

struct Fx {
    dir: TempDir,
    svc: FileOpsService,
    own: Arc<OwnWrites>,
    followers: Arc<Recorder>,
}

impl Fx {
    fn new(files: &[(&str, &str)]) -> Self {
        Self::with(files, |v| v, false)
    }

    fn with(files: &[(&str, &str)], vault: impl FnOnce(Vault) -> Vault, bin_fails: bool) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        for (rel, text) in files {
            let p = dir.path().join(rel);
            fs::create_dir_all(p.parent().expect("parent")).expect("mkdir");
            fs::write(p, text).expect("write");
        }
        let vault = Arc::new(vault(Vault::open(dir.path()).expect("open")));
        let own = Arc::new(OwnWrites::new());
        let followers = Arc::new(Recorder::default());
        let svc = FileOpsService::new(
            Arc::clone(&vault),
            Arc::clone(&own),
            followers.clone(),
            Arc::new(FolderBin {
                bin: tempfile::tempdir().expect("bin"),
                fail: bin_fails,
            }),
        );
        Self {
            dir,
            svc,
            own,
            followers,
        }
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    fn read(&self, rel: &str) -> String {
        fs::read_to_string(self.path(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
    }

    fn exists(&self, rel: &str) -> bool {
        self.path(rel).exists()
    }

    /// Names in a folder as the file system spells them.
    fn names(&self, rel: &str) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.path(rel))
            .expect("read_dir")
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// Previews and applies one move with the links updated.
    fn mv(&self, from: &str, to: &str) -> MoveOutcome {
        let preview = self
            .svc
            .preview_move(&moves(&[(from, to)]), true, &[])
            .expect("preview");
        self.svc
            .apply_move(preview.plan_id, true, &[])
            .expect("apply")
    }
}

fn moves(list: &[(&str, &str)]) -> Vec<Move> {
    list.iter()
        .map(|(from, to)| Move {
            from: (*from).to_owned(),
            to: (*to).to_owned(),
        })
        .collect()
}

fn kind<T: std::fmt::Debug>(r: Result<T, FileOpError>) -> &'static str {
    r.expect_err("an error").kind()
}

const WIKI: [(&str, &str); 4] = [
    (
        "README.md",
        "# Home\n\nSee [the guide](docs/guide.md#setup) and ![logo](img/logo.png).\n",
    ),
    (
        "docs/guide.md",
        "# Guide\n\nBack to [home](../README.md) or [api](api.md).\n",
    ),
    ("docs/api.md", "# API\n"),
    ("img/logo.png", "png"),
];

#[test]
fn renaming_a_file_moves_it_and_rewrites_the_links_to_it() {
    let fx = Fx::new(&WIKI);
    let outcome = fx.mv("docs/guide.md", "docs/manual.md");
    assert_eq!(outcome.moved, moves(&[("docs/guide.md", "docs/manual.md")]));
    assert!(!fx.exists("docs/guide.md") && fx.exists("docs/manual.md"));
    assert!(fx
        .read("README.md")
        .contains("[the guide](docs/manual.md#setup)"));
    // the file's own links did not change: it stayed in its folder
    assert_eq!(
        fx.read("docs/manual.md"),
        "# Guide\n\nBack to [home](../README.md) or [api](api.md).\n"
    );
    assert_eq!((outcome.files_updated, outcome.links_updated), (1, 1));
    assert!(outcome.skipped.is_empty(), "{:?}", outcome.skipped);
    assert_eq!(
        *lock(&fx.followers.renamed),
        [(
            "docs/guide.md".to_owned(),
            "docs/manual.md".to_owned(),
            false
        )]
    );
}

#[test]
fn a_move_into_a_deeper_folder_rewrites_what_the_document_points_at() {
    let fx = Fx::new(&WIKI);
    fs::create_dir_all(fx.path("archive/old")).expect("mkdir");
    let outcome = fx.mv("docs/guide.md", "archive/old/guide.md");
    assert_eq!(
        fx.read("archive/old/guide.md"),
        "# Guide\n\nBack to [home](../../README.md) or [api](../../docs/api.md).\n"
    );
    assert!(fx
        .read("README.md")
        .contains("(archive/old/guide.md#setup)"));
    assert_eq!(outcome.files_updated, 2);
}

#[test]
fn moving_a_folder_keeps_the_links_between_its_files() {
    let fx = Fx::new(&WIKI);
    let outcome = fx.mv("docs", "manual");
    assert!(fx.read("README.md").contains("(manual/guide.md#setup)"));
    // inside the folder nothing needed to change
    assert!(fx.read("manual/guide.md").contains("[api](api.md)"));
    assert_eq!(outcome.files_updated, 1);
    assert_eq!(
        *lock(&fx.followers.renamed),
        [("docs".to_owned(), "manual".to_owned(), true)]
    );
}

#[test]
fn move_without_updating_links_leaves_the_documents_alone() {
    let fx = Fx::new(&WIKI);
    let preview = fx
        .svc
        .preview_move(&moves(&[("docs/guide.md", "docs/manual.md")]), true, &[])
        .expect("preview");
    assert_eq!((preview.link_count, preview.file_count), (1, 1));
    let outcome = fx
        .svc
        .apply_move(preview.plan_id, false, &[])
        .expect("apply");
    assert_eq!((outcome.files_updated, outcome.links_updated), (0, 0));
    assert!(fx.exists("docs/manual.md"));
    assert!(fx.read("README.md").contains("docs/guide.md#setup"));
}

#[test]
fn the_preview_lists_every_link_with_its_line() {
    let fx = Fx::new(&WIKI);
    let preview = fx
        .svc
        .preview_move(&moves(&[("docs/guide.md", "docs/manual.md")]), true, &[])
        .expect("preview");
    assert_eq!(preview.files.len(), 1);
    let file = &preview.files[0];
    assert_eq!(file.path, "README.md");
    assert_eq!(
        file.changes,
        [PreviewChange {
            line: 3,
            old: "docs/guide.md#setup".to_owned(),
            new: "docs/manual.md#setup".to_owned()
        }]
    );
    assert!(!preview.truncated);
    // a preview is a plan only: nothing changed on disk
    assert!(fx.exists("docs/guide.md"));
}

#[test]
fn a_preview_without_link_scanning_reads_no_documents() {
    let fx = Fx::new(&WIKI);
    let preview = fx
        .svc
        .preview_move(&moves(&[("docs/guide.md", "docs/manual.md")]), false, &[])
        .expect("preview");
    assert_eq!((preview.link_count, preview.file_count), (0, 0));
    assert_eq!(preview.moves, moves(&[("docs/guide.md", "docs/manual.md")]));
}

#[test]
fn a_destination_that_exists_is_never_overwritten() {
    let fx = Fx::new(&WIKI);
    assert_eq!(
        kind(
            fx.svc
                .preview_move(&moves(&[("docs/guide.md", "docs/api.md")]), true, &[])
        ),
        "exists"
    );
    // a folder onto a file, a file onto a folder
    assert_eq!(
        kind(
            fx.svc
                .preview_move(&moves(&[("docs", "README.md")]), true, &[])
        ),
        "exists"
    );
    assert_eq!(
        kind(
            fx.svc
                .preview_move(&moves(&[("README.md", "docs")]), true, &[])
        ),
        "exists"
    );
    assert_eq!(fx.read("docs/api.md"), "# API\n");
}

#[test]
fn something_created_between_preview_and_apply_is_not_overwritten_either() {
    let fx = Fx::new(&WIKI);
    let preview = fx
        .svc
        .preview_move(&moves(&[("docs/guide.md", "docs/manual.md")]), true, &[])
        .expect("preview");
    fs::write(fx.path("docs/manual.md"), "someone else's").expect("write");
    assert_eq!(
        kind(fx.svc.apply_move(preview.plan_id, true, &[])),
        "exists"
    );
    assert_eq!(fx.read("docs/manual.md"), "someone else's");
    assert!(fx.exists("docs/guide.md"));
}

#[test]
fn moves_outside_the_project_are_refused() {
    let fx = Fx::new(&WIKI);
    for to in [
        "../outside.md",
        "docs/../../outside.md",
        "/etc/outside.md",
        "C:/outside.md",
    ] {
        let k = kind(fx.svc.preview_move(&moves(&[("README.md", to)]), true, &[]));
        assert!(matches!(k, "pathEscape" | "invalidPath"), "{to}: {k}");
    }
    for from in ["../README.md", "/README.md"] {
        let k = kind(fx.svc.preview_move(&moves(&[(from, "x.md")]), true, &[]));
        assert!(matches!(k, "pathEscape" | "invalidPath"), "{from}: {k}");
    }
    assert!(fx.exists("README.md"));
}

#[test]
fn a_missing_source_or_destination_folder_is_reported() {
    let fx = Fx::new(&WIKI);
    assert_eq!(
        kind(
            fx.svc
                .preview_move(&moves(&[("nope.md", "x.md")]), true, &[])
        ),
        "notFound"
    );
    assert_eq!(
        kind(
            fx.svc
                .preview_move(&moves(&[("README.md", "missing/dir/x.md")]), true, &[])
        ),
        "notFound"
    );
}

#[test]
fn files_do_not_move_between_the_project_and_a_worktree() {
    let wt = tempfile::tempdir().expect("tempdir");
    fs::write(wt.path().join("w.md"), "w").expect("write");
    let path = wt.path().to_path_buf();
    let fx = Fx::with(
        &WIKI,
        move |v| {
            v.with_worktrees(vec![Worktree {
                name: "feat".to_owned(),
                path,
                branch: None,
            }])
        },
        false,
    );
    assert_eq!(
        kind(
            fx.svc
                .preview_move(&moves(&[("README.md", "@wt/feat/README.md")]), true, &[])
        ),
        "otherRoot"
    );
    assert_eq!(
        kind(
            fx.svc
                .preview_move(&moves(&[("@wt/feat/w.md", "w.md")]), true, &[])
        ),
        "otherRoot"
    );
    // the worktree folder itself is not an item that can be renamed or moved
    assert_eq!(
        kind(
            fx.svc
                .preview_move(&moves(&[("@wt/feat", "@wt/other")]), true, &[])
        ),
        "protected"
    );
    // inside the worktree, a rename works
    let preview = fx
        .svc
        .preview_move(&moves(&[("@wt/feat/w.md", "@wt/feat/v.md")]), true, &[])
        .expect("preview");
    fx.svc
        .apply_move(preview.plan_id, true, &[])
        .expect("apply");
    assert!(wt.path().join("v.md").exists());
}

#[test]
fn roots_and_the_git_folder_are_protected() {
    let fx = Fx::new(&[("a.md", "x"), (".git/config", "x")]);
    for (from, to) in [
        ("", "x"),
        (".git", "git2"),
        ("a.md", ".git/a.md"),
        ("a.md", ".trailmark"),
    ] {
        assert_eq!(
            kind(fx.svc.preview_move(&moves(&[(from, to)]), true, &[])),
            "protected",
            "{from} -> {to}"
        );
    }
    assert_eq!(kind(fx.svc.delete(&[".git".to_owned()])), "protected");
    assert_eq!(kind(fx.svc.delete(&[String::new()])), "protected");
}

#[test]
fn a_folder_cannot_be_moved_into_itself_and_names_are_checked() {
    let fx = Fx::new(&WIKI);
    assert_eq!(
        kind(
            fx.svc
                .preview_move(&moves(&[("docs", "docs/inner")]), true, &[])
        ),
        "intoItself"
    );
    for bad in [
        "bad:name.md",
        "what?.md",
        "trailing.",
        " lead.md",
        "con.md",
        "",
    ] {
        let k = kind(
            fx.svc
                .preview_move(&moves(&[("README.md", bad)]), true, &[]),
        );
        assert!(
            matches!(k, "invalidName" | "protected" | "invalidPath"),
            "{bad:?}: {k}"
        );
    }
    assert_eq!(
        kind(
            fx.svc
                .preview_move(&moves(&[("README.md", "README.md")]), true, &[])
        ),
        "nothing"
    );
}

#[test]
fn a_name_that_is_kept_is_not_checked_again() {
    // `.editorconfig` could not be given as a new name, but moving it keeps it
    let fx = Fx::new(&[(".editorconfig", "root = true"), ("sub/keep.md", "x")]);
    fx.mv(".editorconfig", "sub/.editorconfig");
    assert!(fx.exists("sub/.editorconfig"));
}

#[test]
fn several_moves_happen_together_and_must_not_overlap() {
    let fx = Fx::new(&WIKI);
    fs::create_dir_all(fx.path("archive")).expect("mkdir");
    let both = moves(&[
        ("docs/guide.md", "archive/guide.md"),
        ("docs/api.md", "archive/api.md"),
    ]);
    let preview = fx.svc.preview_move(&both, true, &[]).expect("preview");
    let outcome = fx
        .svc
        .apply_move(preview.plan_id, true, &[])
        .expect("apply");
    assert_eq!(outcome.moved.len(), 2);
    assert_eq!(fx.names("archive"), ["api.md", "guide.md"]);
    assert!(fx.read("README.md").contains("(archive/guide.md#setup)"));
    // the two moved together keep the link between them
    assert!(fx.read("archive/guide.md").contains("[api](api.md)"));

    let fx = Fx::new(&WIKI);
    assert_eq!(
        kind(fx.svc.preview_move(
            &moves(&[("docs", "manual"), ("docs/guide.md", "guide.md")]),
            true,
            &[]
        )),
        "overlap"
    );
    assert_eq!(
        kind(fx.svc.preview_move(
            &moves(&[("docs/guide.md", "same.md"), ("docs/api.md", "same.md")]),
            true,
            &[]
        )),
        "exists"
    );
    assert_eq!(kind(fx.svc.preview_move(&[], true, &[])), "nothing");
}

#[test]
fn a_move_that_fails_half_way_puts_back_what_it_moved() {
    let fx = Fx::new(&WIKI);
    let two = moves(&[("docs/guide.md", "g.md"), ("docs/api.md", "a.md")]);
    let preview = fx.svc.preview_move(&two, true, &[]).expect("preview");
    // the second source disappears before the move is made
    fs::remove_file(fx.path("docs/api.md")).expect("remove");
    assert!(fx.svc.apply_move(preview.plan_id, true, &[]).is_err());
    assert!(fx.exists("docs/guide.md") && !fx.exists("g.md"));
}

#[test]
fn a_preview_can_be_applied_once() {
    let fx = Fx::new(&WIKI);
    let preview = fx
        .svc
        .preview_move(&moves(&[("docs/guide.md", "docs/manual.md")]), true, &[])
        .expect("preview");
    fx.svc
        .apply_move(preview.plan_id, true, &[])
        .expect("apply");
    assert_eq!(
        kind(fx.svc.apply_move(preview.plan_id, true, &[])),
        "unknownPlan"
    );
    assert_eq!(kind(fx.svc.apply_move(9999, true, &[])), "unknownPlan");
}

#[test]
fn a_case_only_rename_works() {
    let fx = Fx::new(&[
        ("docs/readme.md", "# R\n"),
        ("a.md", "[r](docs/readme.md)\n"),
    ]);
    fx.mv("docs/readme.md", "docs/README.md");
    assert_eq!(fx.names("docs"), ["README.md"]);
    assert_eq!(fx.read("a.md"), "[r](docs/README.md)\n");
    // a folder too
    fx.mv("docs", "Docs");
    assert_eq!(fx.names(""), ["Docs", "a.md"]);
    assert_eq!(fx.read("a.md"), "[r](Docs/README.md)\n");
}

#[test]
fn a_document_that_changed_after_the_preview_is_skipped_and_reported() {
    let fx = Fx::new(&[
        ("a.md", "[x](g.md)\n"),
        ("b.md", "[x](g.md)\n"),
        ("g.md", "g"),
    ]);
    let preview = fx
        .svc
        .preview_move(&moves(&[("g.md", "h.md")]), true, &[])
        .expect("preview");
    assert_eq!(preview.file_count, 2);
    fs::write(fx.path("a.md"), "[x](g.md)\nan agent wrote this\n").expect("write");
    let outcome = fx
        .svc
        .apply_move(preview.plan_id, true, &[])
        .expect("apply");
    assert_eq!(
        outcome.skipped,
        [SkippedFile {
            path: "a.md".to_owned(),
            reason: SkipReason::Changed
        }]
    );
    assert_eq!(fx.read("a.md"), "[x](g.md)\nan agent wrote this\n");
    assert_eq!(fx.read("b.md"), "[x](h.md)\n");
    assert!(fx.exists("h.md"), "the move itself is made");
}

#[test]
fn a_document_with_unsaved_edits_is_not_written_and_gets_edits_for_its_buffer() {
    let fx = Fx::new(&[
        ("a.md", "[x](g.md)\n"),
        ("b.md", "[x](g.md)\n"),
        ("g.md", "g"),
    ]);
    let preview = fx
        .svc
        .preview_move(&moves(&[("g.md", "sub/h.md")]), true, &[]);
    // the destination folder does not exist yet
    assert_eq!(kind(preview), "notFound");
    fs::create_dir(fx.path("sub")).expect("mkdir");
    let preview = fx
        .svc
        .preview_move(&moves(&[("g.md", "sub/h.md")]), true, &[])
        .expect("preview");
    // the editor holds a.md with a line the file does not have, and a multi-byte character before the link
    let buffer = BufferText {
        path: "a.md".to_owned(),
        text: "Olá 😀 [x](g.md)\n\nmore [y](g.md#top)\n".to_owned(),
    };
    let outcome = fx
        .svc
        .apply_move(preview.plan_id, true, std::slice::from_ref(&buffer))
        .expect("apply");
    assert_eq!(
        fx.read("a.md"),
        "[x](g.md)\n",
        "the file on disk is untouched"
    );
    assert_eq!(fx.read("b.md"), "[x](sub/h.md)\n");
    assert_eq!(outcome.buffers.len(), 1);
    let edits = &outcome.buffers[0];
    assert_eq!(
        (edits.path.as_str(), edits.new_path.as_str()),
        ("a.md", "a.md")
    );
    // offsets count UTF-16 units: the emoji is two
    let units: Vec<u16> = buffer.text.encode_utf16().collect();
    let mut text = units.clone();
    for e in edits.edits.iter().rev() {
        text.splice(e.from..e.to, e.insert.encode_utf16());
    }
    assert_eq!(
        String::from_utf16(&text).expect("utf16"),
        "Olá 😀 [x](sub/h.md)\n\nmore [y](sub/h.md#top)\n"
    );
}

#[test]
fn the_buffer_of_a_document_that_moves_gets_its_own_links_recomputed() {
    let fx = Fx::new(&[("docs/a.md", "[r](../README.md)\n"), ("README.md", "x")]);
    fs::create_dir_all(fx.path("deeper/more")).expect("mkdir");
    let preview = fx
        .svc
        .preview_move(&moves(&[("docs/a.md", "deeper/more/a.md")]), true, &[])
        .expect("preview");
    let buffer = BufferText {
        path: "docs/a.md".to_owned(),
        text: "[r](../README.md) unsaved".to_owned(),
    };
    let outcome = fx
        .svc
        .apply_move(preview.plan_id, true, &[buffer])
        .expect("apply");
    assert_eq!(outcome.buffers[0].new_path, "deeper/more/a.md");
    assert_eq!(
        fx.read("deeper/more/a.md"),
        "[r](../README.md)\n",
        "disk untouched"
    );
    assert_eq!(outcome.buffers[0].edits[0].insert, "../../README.md");
}

#[test]
fn a_held_back_document_is_left_out_of_the_preview_and_listed_as_skipped() {
    let fx = Fx::new(&[
        ("a.md", "[x](g.md)\n"),
        ("b.md", "[x](g.md) and [y](g.md#top)\n"),
        ("c.md", "no links here\n"),
        ("g.md", "g"),
    ]);
    let held = ["b.md".to_owned(), "c.md".to_owned()];
    let preview = fx
        .svc
        .preview_move(&moves(&[("g.md", "h.md")]), true, &held)
        .expect("preview");
    // only a.md is left to rewrite; b.md had links, c.md had none and is not mentioned
    assert_eq!((preview.link_count, preview.file_count), (1, 1));
    assert_eq!(
        preview
            .files
            .iter()
            .map(|f| f.path.as_str())
            .collect::<Vec<_>>(),
        ["a.md"]
    );
    assert_eq!(
        preview.skipped,
        [SkippedFile {
            path: "b.md".to_owned(),
            reason: SkipReason::UnsavedVisual
        }]
    );

    // applying leaves the held document's file alone and carries the note into the result
    let outcome = fx
        .svc
        .apply_move(preview.plan_id, true, &[])
        .expect("apply");
    assert_eq!(fx.read("a.md"), "[x](h.md)\n");
    assert_eq!(fx.read("b.md"), "[x](g.md) and [y](g.md#top)\n");
    assert!(fx.exists("h.md"), "the move itself is made");
    assert_eq!((outcome.files_updated, outcome.links_updated), (1, 1));
    assert_eq!(outcome.skipped, preview.skipped);
}

#[test]
fn a_held_back_document_that_moves_itself_keeps_its_links_for_later() {
    let fx = Fx::new(&[("docs/a.md", "[r](../README.md)\n"), ("README.md", "x")]);
    fs::create_dir_all(fx.path("deeper/more")).expect("mkdir");
    let preview = fx
        .svc
        .preview_move(
            &moves(&[("docs/a.md", "deeper/more/a.md")]),
            true,
            &["docs/a.md".to_owned()],
        )
        .expect("preview");
    assert_eq!((preview.link_count, preview.file_count), (0, 0));
    assert_eq!(
        preview.skipped,
        [SkippedFile {
            path: "docs/a.md".to_owned(),
            reason: SkipReason::UnsavedVisual
        }]
    );
    let outcome = fx
        .svc
        .apply_move(preview.plan_id, true, &[])
        .expect("apply");
    assert_eq!(fx.read("deeper/more/a.md"), "[r](../README.md)\n");
    assert_eq!(outcome.files_updated, 0);
}

#[test]
fn nothing_is_held_back_without_a_matching_path() {
    let fx = Fx::new(&WIKI);
    let plain = fx
        .svc
        .preview_move(&moves(&[("docs/guide.md", "docs/manual.md")]), true, &[])
        .expect("preview");
    let other = fx
        .svc
        .preview_move(
            &moves(&[("docs/guide.md", "docs/manual.md")]),
            true,
            &["docs/api.md".to_owned(), "nope.md".to_owned()],
        )
        .expect("preview");
    assert_eq!((other.link_count, other.file_count), (1, 1));
    assert_eq!(other.files, plain.files);
    assert_eq!(other.skipped, plain.skipped);
}

#[test]
fn undo_moves_everything_back_and_restores_the_links() {
    let fx = Fx::new(&WIKI);
    let before: Vec<String> = ["README.md", "docs/guide.md", "docs/api.md"]
        .iter()
        .map(|p| fx.read(p))
        .collect();
    let outcome = fx.mv("docs", "manual");
    assert!(fx.read("README.md").contains("manual/guide.md"));

    let undone = fx.svc.undo(outcome.op_id).expect("undo");
    assert_eq!(undone.moved_back, moves(&[("manual", "docs")]));
    assert_eq!((undone.files_restored, undone.links_restored), (1, 1));
    assert!(undone.could_not.is_empty(), "{:?}", undone.could_not);
    let after: Vec<String> = ["README.md", "docs/guide.md", "docs/api.md"]
        .iter()
        .map(|p| fx.read(p))
        .collect();
    assert_eq!(after, before);
    assert!(!fx.exists("manual"));
    // the followers heard both ways
    let heard = lock(&fx.followers.renamed).clone();
    assert_eq!(heard.len(), 2);
    assert_eq!(
        (heard[1].0.as_str(), heard[1].1.as_str()),
        ("manual", "docs")
    );
    // an operation is undone once
    assert_eq!(kind(fx.svc.undo(outcome.op_id)), "unknownOperation");
}

#[test]
fn undo_of_a_moved_document_with_rewritten_links_restores_both() {
    let fx = Fx::new(&WIKI);
    fs::create_dir_all(fx.path("archive/old")).expect("mkdir");
    let original = fx.read("docs/guide.md");
    let outcome = fx.mv("docs/guide.md", "archive/old/guide.md");
    assert_ne!(fx.read("archive/old/guide.md"), original);
    let undone = fx.svc.undo(outcome.op_id).expect("undo");
    assert_eq!(undone.files_restored, 2);
    assert_eq!(fx.read("docs/guide.md"), original);
    assert!(fx.read("README.md").contains("docs/guide.md#setup"));
}

#[test]
fn undo_reports_what_changed_since_instead_of_overwriting_it() {
    let fx = Fx::new(&WIKI);
    let outcome = fx.mv("docs/guide.md", "docs/manual.md");
    // someone edits the document whose links were rewritten
    fs::write(
        fx.path("README.md"),
        "# rewritten by someone\n[x](docs/manual.md)\n",
    )
    .expect("write");
    let undone = fx.svc.undo(outcome.op_id).expect("undo");
    assert_eq!(
        undone.could_not,
        [SkippedFile {
            path: "README.md".to_owned(),
            reason: SkipReason::Changed
        }]
    );
    assert_eq!(
        fx.read("README.md"),
        "# rewritten by someone\n[x](docs/manual.md)\n"
    );
    // the file itself still went back
    assert!(fx.exists("docs/guide.md"));
}

#[test]
fn undo_does_not_put_a_file_where_another_one_has_appeared() {
    let fx = Fx::new(&WIKI);
    let outcome = fx.mv("docs/guide.md", "docs/manual.md");
    fs::write(fx.path("docs/guide.md"), "a new file").expect("write");
    let undone = fx.svc.undo(outcome.op_id).expect("undo");
    assert!(undone.moved_back.is_empty(), "{:?}", undone.moved_back);
    // the reason is that something is in the way, not that a write failed
    assert_eq!(
        undone.could_not,
        [SkippedFile {
            path: "docs/manual.md".to_owned(),
            reason: SkipReason::Occupied
        }]
    );
    assert_eq!(fx.read("docs/guide.md"), "a new file");
    assert!(fx.exists("docs/manual.md"));
    // the link keeps leading to the document, which is still at its new place, and not to the new file
    assert_eq!((undone.files_restored, undone.links_restored), (0, 0));
    assert!(fx
        .read("README.md")
        .contains("[the guide](docs/manual.md#setup)"));
}

#[test]
fn undo_puts_back_the_links_of_the_files_that_go_back_and_only_those() {
    let fx = Fx::new(&[
        ("README.md", "# Home\n[a](a.md) and [b](b.md)\n"),
        ("a.md", "# A\n"),
        ("b.md", "# B\n"),
        ("archive/keep.md", "k"),
    ]);
    let both = moves(&[("a.md", "archive/a.md"), ("b.md", "archive/b.md")]);
    let preview = fx.svc.preview_move(&both, true, &[]).expect("preview");
    let outcome = fx
        .svc
        .apply_move(preview.plan_id, true, &[])
        .expect("apply");
    assert_eq!(
        fx.read("README.md"),
        "# Home\n[a](archive/a.md) and [b](archive/b.md)\n"
    );
    // a new a.md appears where the old one was
    fs::write(fx.path("a.md"), "a new file").expect("write");
    let undone = fx.svc.undo(outcome.op_id).expect("undo");
    assert_eq!(undone.moved_back, moves(&[("archive/b.md", "b.md")]));
    assert_eq!(
        undone.could_not,
        [SkippedFile {
            path: "archive/a.md".to_owned(),
            reason: SkipReason::Occupied
        }]
    );
    assert_eq!((undone.files_restored, undone.links_restored), (1, 1));
    assert_eq!(
        fx.read("README.md"),
        "# Home\n[a](archive/a.md) and [b](b.md)\n"
    );
    assert!(fx.exists("archive/a.md") && fx.exists("b.md") && !fx.exists("archive/b.md"));
}

#[test]
fn a_moved_document_that_cannot_go_back_keeps_the_links_it_was_given() {
    let fx = Fx::new(&WIKI);
    fs::create_dir_all(fx.path("archive/old")).expect("mkdir");
    let outcome = fx.mv("docs/guide.md", "archive/old/guide.md");
    let moved_text = fx.read("archive/old/guide.md");
    fs::write(fx.path("docs/guide.md"), "a new file").expect("write");
    let undone = fx.svc.undo(outcome.op_id).expect("undo");
    assert_eq!(undone.moved_back, moves(&[]));
    assert_eq!(undone.files_restored, 0);
    // its own links still work from where it stays, and so does the link that leads to it
    assert_eq!(fx.read("archive/old/guide.md"), moved_text);
    assert!(fx
        .read("README.md")
        .contains("(archive/old/guide.md#setup)"));
}

#[test]
fn deleting_sends_files_and_folders_to_the_bin() {
    let fx = Fx::new(&WIKI);
    let outcome = fx
        .svc
        .delete(&[
            "docs".to_owned(),
            "docs/api.md".to_owned(),
            "img/logo.png".to_owned(),
        ])
        .expect("delete");
    // the file inside the deleted folder goes with its folder
    assert_eq!(outcome.deleted, ["docs", "img/logo.png"]);
    assert!(!fx.exists("docs") && !fx.exists("img/logo.png") && fx.exists("README.md"));
}

#[test]
fn deleting_things_that_do_not_exist_or_leave_the_project_is_refused() {
    let fx = Fx::new(&WIKI);
    assert_eq!(kind(fx.svc.delete(&["nope.md".to_owned()])), "notFound");
    let k = kind(fx.svc.delete(&["../x.md".to_owned()]));
    assert!(matches!(k, "pathEscape" | "invalidPath"), "{k}");
    assert_eq!(kind(fx.svc.delete(&[])), "nothing");
    assert!(fx.exists("README.md"));
}

#[test]
fn a_bin_that_fails_leaves_the_files_where_they_are() {
    let fx = Fx::with(&WIKI, |v| v, true);
    assert_eq!(kind(fx.svc.delete(&["README.md".to_owned()])), "trash");
    assert!(fx.exists("README.md"));
}

#[test]
fn a_bin_that_fails_does_not_hide_a_real_removal_from_the_feed() {
    let fx = Fx::with(&WIKI, |v| v, true);
    assert_eq!(kind(fx.svc.delete(&["README.md".to_owned()])), "trash");
    // someone else removes the file a moment later: that is news, the failed attempt was not an announcement
    let mut changes = vec![FsChange {
        kind: ChangeKind::Removed,
        path: "README.md".to_owned(),
        from: None,
        at_ms: 1,
    }];
    fx.own.strip(&mut changes, &SigStore::new());
    assert_eq!(changes.len(), 1);
}

#[test]
fn a_folder_with_a_linked_worktree_in_it_is_not_moved_or_deleted() {
    let fx = Fx::new(&WIKI);
    // a worktree that lives inside the project, in a folder of its own
    let inner = fx.path("agents/trees/x");
    fs::create_dir_all(&inner).expect("mkdir");
    fs::write(inner.join("code.rs"), "x").expect("write");
    let vault = Vault::open(fx.dir.path())
        .expect("open")
        .with_worktrees(vec![Worktree {
            name: "x".to_owned(),
            path: inner,
            branch: None,
        }]);
    let svc = FileOpsService::new(
        Arc::new(vault),
        Arc::clone(&fx.own),
        fx.followers.clone(),
        Arc::new(FolderBin {
            bin: tempfile::tempdir().expect("bin"),
            fail: false,
        }),
    );
    assert_eq!(
        kind(svc.preview_move(&moves(&[("agents", "elsewhere")]), true, &[])),
        "protected"
    );
    assert_eq!(kind(svc.delete(&["agents".to_owned()])), "protected");
    assert!(fx.exists("agents/trees/x/code.rs"));
}

#[test]
fn a_new_folder_is_made_and_can_be_taken_back_while_empty() {
    let fx = Fx::new(&WIKI);
    let made = fx.svc.new_folder("docs/new folder").expect("folder");
    assert_eq!(made.path, "docs/new folder");
    assert!(fx.path("docs/new folder").is_dir());
    let undone = fx.svc.undo(made.op_id).expect("undo");
    assert_eq!(undone.folders_removed, 1);
    assert!(!fx.exists("docs/new folder"));

    let made = fx.svc.new_folder("docs/again").expect("folder");
    fs::write(fx.path("docs/again/x.md"), "x").expect("write");
    let undone = fx.svc.undo(made.op_id).expect("undo");
    assert_eq!(undone.folders_removed, 0);
    assert_eq!(
        undone.could_not.len(),
        1,
        "a folder with something in it stays"
    );
}

#[test]
fn new_folders_are_checked_like_every_other_path() {
    let fx = Fx::new(&WIKI);
    assert_eq!(kind(fx.svc.new_folder("docs")), "exists");
    assert_eq!(kind(fx.svc.new_folder("README.md")), "exists");
    assert_eq!(kind(fx.svc.new_folder("a/b/c")), "notFound");
    assert_eq!(kind(fx.svc.new_folder("bad|name")), "invalidName");
    assert_eq!(kind(fx.svc.new_folder(".git")), "protected");
    let k = kind(fx.svc.new_folder("../outside"));
    assert!(matches!(k, "pathEscape" | "invalidPath"), "{k}");
}

/// What the watcher reports for the moves of this module's operations must not reach the feed.
#[test]
fn the_reports_of_a_move_and_its_rewrites_are_announced_to_the_watcher() {
    let fx = Fx::new(&WIKI);
    fx.mv("docs/guide.md", "docs/manual.md");
    let readme_hash = hash_bytes(fx.read("README.md").as_bytes());
    let sigs = SigStore::new();
    sigs.set(
        key("README.md"),
        "README.md",
        FileSig {
            len: 1,
            mtime_ms: 1,
            hash: readme_hash,
        },
    );
    let change = |kind: ChangeKind, path: &str| FsChange {
        kind,
        path: path.to_owned(),
        from: None,
        at_ms: 1,
    };
    let mut changes = vec![
        change(ChangeKind::Removed, "docs/guide.md"),
        change(ChangeKind::Created, "docs/manual.md"),
        change(ChangeKind::Modified, "README.md"),
        change(ChangeKind::Modified, "docs/api.md"),
    ];
    fx.own.strip(&mut changes, &sigs);
    // an unrelated edit stays news
    assert_eq!(changes, [change(ChangeKind::Modified, "docs/api.md")]);
}

#[test]
fn only_the_files_of_a_moved_folder_are_announced_not_what_an_agent_adds_later() {
    let fx = Fx::new(&WIKI);
    fx.mv("docs", "manual");
    let sigs = SigStore::new();
    let mut changes: Vec<FsChange> = ["docs/guide.md", "docs/api.md"]
        .iter()
        .map(|p| FsChange {
            kind: ChangeKind::Removed,
            path: (*p).to_owned(),
            from: None,
            at_ms: 1,
        })
        .chain(
            [
                "manual/guide.md",
                "manual/api.md",
                "manual/new-from-an-agent.md",
            ]
            .iter()
            .map(|p| FsChange {
                kind: ChangeKind::Created,
                path: (*p).to_owned(),
                from: None,
                at_ms: 1,
            }),
        )
        .collect();
    fx.own.strip(&mut changes, &sigs);
    let left: Vec<&str> = changes.iter().map(|c| c.path.as_str()).collect();
    assert_eq!(left, ["manual/new-from-an-agent.md"]);
}

#[test]
fn a_document_that_cannot_be_read_as_text_is_reported_in_the_preview() {
    let fx = Fx::new(&[("g.md", "g")]);
    fs::write(fx.path("latin.md"), b"[x](g.md) caf\xE9").expect("write");
    let preview = fx
        .svc
        .preview_move(&moves(&[("g.md", "h.md")]), true, &[])
        .expect("preview");
    assert_eq!(
        preview.skipped,
        [SkippedFile {
            path: "latin.md".to_owned(),
            reason: SkipReason::NotUtf8
        }]
    );
    let outcome = fx
        .svc
        .apply_move(preview.plan_id, true, &[])
        .expect("apply");
    assert_eq!(outcome.skipped.len(), 1);
}

#[test]
fn the_documents_of_a_gitignored_folder_are_not_touched() {
    let fx = Fx::new(&[
        (".gitignore", "build/\n"),
        ("g.md", "g"),
        ("build/out.md", "[x](../g.md)\n"),
        ("a.md", "[x](g.md)\n"),
    ]);
    fx.mv("g.md", "h.md");
    assert_eq!(fx.read("a.md"), "[x](h.md)\n");
    assert_eq!(fx.read("build/out.md"), "[x](../g.md)\n");
}
