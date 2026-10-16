use std::collections::BTreeMap;
use std::fs;

use super::*;
use crate::unread::UnreadService;
use crate::watch::sigs::FileSig;
use crate::watch::{ChangeKind, FsChange, PathFilter, SigStore};

/// Waits are not part of these tests: the thread answers `flush` only after everything sent before it.
const NEVER: Timing = Timing {
    quiet: Duration::from_secs(3600),
    max_wait: Duration::from_secs(7200),
    retry: Duration::from_secs(3600),
};

const V0: &str = "# Plan\n\nThe launch is planned for June and the team is ready.\n\nBudget stays the same.\n\n- one\n- two\n";
const V1: &str = "# Plan\n\nThe launch is planned for July and the team is ready.\n\nBudget stays the same.\n\n- one\n- two\n- three\n\nExtra note added by the agent.\n";

fn h(text: &str) -> u64 {
    hash_bytes(text.as_bytes())
}

fn hex(text: &str) -> String {
    hash_to_hex(h(text))
}

fn doc(path: &str, current: Option<&str>, read: Option<&str>) -> DocState {
    DocState {
        path: path.to_owned(),
        current: current.map(h),
        read: read.map(h),
    }
}

/// What the UI hands back: the hash of the file and of the approved version.
fn basis(file: Option<&str>, reviewed: Option<&str>) -> Basis {
    Basis {
        hash: file.map(hex),
        reviewed: reviewed.map(hex),
    }
}

fn entry(path: &str, status: Status) -> Entry {
    Entry {
        path: path.to_owned(),
        status,
    }
}

/// A project in a folder, a place for the review's files, and what the review announced.
struct Fx {
    project: tempfile::TempDir,
    state: tempfile::TempDir,
    heard: Arc<Mutex<Vec<Vec<Entry>>>>,
    own: Arc<OwnWrites>,
}

/// Committed texts for a test that has no repository.
struct Commits(HashMap<String, Vec<u8>>);

impl Committed for Commits {
    fn blob(&self, path: &str) -> Option<Vec<u8>> {
        self.0.get(path).cloned()
    }
}

/// The service hears the documents, and has dealt with it once this returns.
fn tell(service: &ReviewService, docs: &[DocState]) {
    service.changed(Vec::new(), true, docs.to_vec());
    service.flush();
}

impl Fx {
    fn new(files: &[(&str, &str)]) -> Self {
        let fx = Self {
            project: tempfile::tempdir().expect("tempdir"),
            state: tempfile::tempdir().expect("tempdir"),
            heard: Arc::default(),
            own: Arc::new(OwnWrites::new()),
        };
        for (path, text) in files {
            fx.write(path, text);
        }
        fx
    }

    fn write(&self, rel: &str, text: &str) {
        let path = self.project.path().join(rel);
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(path, text).expect("write");
    }

    fn remove(&self, rel: &str) {
        fs::remove_file(self.project.path().join(rel)).expect("remove");
    }

    fn read(&self, rel: &str) -> String {
        fs::read_to_string(self.project.path().join(rel)).expect("read")
    }

    fn paths(&self) -> Paths {
        Paths {
            state: self.state.path().join("reviewed.json"),
            copies: self.state.path().join("reviewed"),
        }
    }

    fn vault(&self) -> Arc<Vault> {
        Arc::new(Vault::open(self.project.path()).expect("vault"))
    }

    fn service(&self) -> ReviewService {
        self.service_with(None)
    }

    fn service_with(&self, committed: Option<Arc<dyn Committed>>) -> ReviewService {
        let heard = Arc::clone(&self.heard);
        ReviewService::open(
            self.vault(),
            Arc::clone(&self.own),
            self.paths(),
            committed,
            NEVER,
            Arc::new(move |entries: &[Entry]| lock(&heard).push(entries.to_vec())),
        )
    }

    /// The service hears the person read a document (what it holds now) and then the documents.
    fn read_by_person(&self, service: &ReviewService, path: &str, docs: &[DocState]) {
        let bytes = fs::read(self.project.path().join(path)).expect("read");
        service.reading(path, hash_bytes(&bytes), &bytes);
        tell(service, docs);
    }

    fn copies(&self) -> HashSet<u64> {
        CopyStore::new(self.paths().copies).hashes()
    }

    /// The approved version of each document, as `reviewed.json` has it.
    fn saved(&self) -> Option<BTreeMap<String, String>> {
        let bytes = fs::read(self.paths().state).ok()?;
        let file: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
        assert_eq!(file["version"], 1);
        file["documents"]
            .as_object()?
            .iter()
            .map(|(path, doc)| Some((path.clone(), doc["hash"].as_str()?.to_owned())))
            .collect()
    }

    fn last_heard(&self) -> Option<Vec<Entry>> {
        lock(&self.heard).last().cloned()
    }

    /// A project where `a.md` was read as `V0` and an agent has since written `V1`.
    fn agent_changed_a(&self) -> ReviewService {
        self.write("a.md", V0);
        let service = self.service();
        tell(&service, &[doc("a.md", Some(V0), Some(V0))]);
        self.write("a.md", V1);
        tell(&service, &[doc("a.md", Some(V1), Some(V0))]);
        service
    }
}

// ---- the approved version and its copy ----

#[test]
fn what_was_read_is_approved_and_a_copy_of_it_is_kept_and_remembered() {
    let fx = Fx::new(&[("a.md", V0), ("b.md", "# B\n")]);
    let service = fx.service();
    tell(
        &service,
        &[
            doc("a.md", Some(V0), Some(V0)),
            doc("b.md", Some("# B\n"), Some("# B\n")),
        ],
    );
    assert_eq!(fx.copies(), HashSet::from([h(V0), h("# B\n")]));
    assert_eq!(
        fx.saved().expect("reviewed.json"),
        BTreeMap::from([
            ("a.md".to_owned(), hex(V0)),
            ("b.md".to_owned(), hex("# B\n"))
        ])
    );
    assert_eq!(service.entries(), Vec::<Entry>::new());
    service.stop();
}

#[test]
fn the_approved_version_of_a_document_that_changed_is_still_the_one_that_was_read() {
    let fx = Fx::new(&[]);
    let service = fx.agent_changed_a();
    assert_eq!(service.entries(), [entry("a.md", Status::Changed)]);
    assert_eq!(fx.last_heard(), Some(vec![entry("a.md", Status::Changed)]));
    assert!(fx.copies().contains(&h(V0)), "and its copy is kept");
    service.stop();
}

#[test]
fn a_version_without_a_copy_is_taken_from_the_disk_when_the_file_still_has_it() {
    // the first run after an update: reviewed.json does not exist, read.json says what was read
    let fx = Fx::new(&[("a.md", V0)]);
    let service = fx.service();
    tell(&service, &[doc("a.md", Some(V0), Some(V0))]);
    assert_eq!(fx.copies(), HashSet::from([h(V0)]));
    service.stop();
}

#[test]
fn a_version_without_a_copy_is_taken_from_the_last_commit_when_the_file_has_changed() {
    let fx = Fx::new(&[("a.md", V1)]);
    let commits = Commits(HashMap::from([("a.md".to_owned(), V0.as_bytes().to_vec())]));
    let service = fx.service_with(Some(Arc::new(commits)));
    tell(&service, &[doc("a.md", Some(V1), Some(V0))]);
    assert_eq!(fx.copies(), HashSet::from([h(V0)]));
    let view = service.view("a.md").expect("view");
    assert_eq!(view.status, ViewStatus::Changes);
    assert_eq!(view.changes.len(), 2);
    service.stop();
}

#[test]
fn a_commit_that_does_not_have_the_version_read_gives_no_baseline() {
    let fx = Fx::new(&[("a.md", V1)]);
    let commits = Commits(HashMap::from([(
        "a.md".to_owned(),
        b"# Some other version\n".to_vec(),
    )]));
    let service = fx.service_with(Some(Arc::new(commits)));
    tell(&service, &[doc("a.md", Some(V1), Some(V0))]);
    assert_eq!(fx.copies(), HashSet::new());
    let view = service.view("a.md").expect("view");
    assert_eq!(view.status, ViewStatus::NoBaseline);
    assert_eq!(view.hash, Some(hex(V1)));
    assert_eq!(view.reviewed_hash, Some(hex(V0)));
    // reading it makes the version it holds the approved one, and then it can be reviewed from its next change
    fx.read_by_person(&service, "a.md", &[doc("a.md", Some(V1), Some(V1))]);
    assert_eq!(
        service.view("a.md").expect("view").status,
        ViewStatus::Unchanged
    );
    service.stop();
}

#[test]
fn a_document_the_person_saved_is_approved_and_its_copy_is_kept() {
    let fx = Fx::new(&[("a.md", V0)]);
    let service = fx.service();
    tell(&service, &[doc("a.md", Some(V0), Some(V0))]);
    fx.write("a.md", "# Mine\n");
    service.wrote(b"# Mine\n");
    tell(&service, &[doc("a.md", Some("# Mine\n"), Some("# Mine\n"))]);
    assert!(fx.copies().contains(&h("# Mine\n")));
    assert_eq!(service.entries(), Vec::<Entry>::new());
    service.stop();
}

#[test]
fn copies_nothing_points_to_are_deleted_but_not_those_just_made() {
    let fx = Fx::new(&[]);
    let service = fx.agent_changed_a();
    // all documents are marked read (nobody opened this one): V1 is approved, and V0 is let go
    tell(&service, &[doc("a.md", Some(V1), Some(V1))]);
    assert!(
        fx.copies().contains(&h(V0)),
        "just made, so kept for a while"
    );
    service.age_copies();
    service.flush();
    assert_eq!(fx.copies(), HashSet::from([h(V1)]));
    service.stop();
}

#[test]
fn a_copy_in_use_survives_the_clean_up() {
    let fx = Fx::new(&[]);
    let service = fx.agent_changed_a();
    service.age_copies();
    service.flush();
    assert!(fx.copies().contains(&h(V0)));
    service.stop();
}

#[test]
fn what_was_approved_is_remembered_by_the_next_run() {
    let fx = Fx::new(&[]);
    let service = fx.agent_changed_a();
    let basis = basis(Some(V1), Some(V0));
    service.keep("a.md", &basis, 0).expect("keep");
    service.stop();

    // the next run: the person kept the first change, then closed the app; reading says V0 still
    let again = fx.service();
    tell(&again, &[doc("a.md", Some(V1), Some(V0))]);
    let view = again.view("a.md").expect("view");
    assert_eq!(view.status, ViewStatus::Changes);
    assert_eq!(view.changes.len(), 1, "only the second change is left");
    again.stop();
}

// ---- looking at a document ----

#[test]
fn a_changed_document_is_shown_change_by_change_with_the_hashes_it_was_drawn_from() {
    let fx = Fx::new(&[]);
    let service = fx.agent_changed_a();
    let view = service.view("a.md").expect("view");
    assert_eq!(view.status, ViewStatus::Changes);
    assert_eq!(view.hash, Some(hex(V1)));
    assert_eq!(view.reviewed_hash, Some(hex(V0)));
    assert_eq!(view.changes.len(), 2);
    let first = &view.changes[0].rows[0];
    assert!(first.old.as_ref().expect("old").html.contains("June"));
    assert!(first.new.as_ref().expect("new").html.contains("July"));
    assert_eq!(
        view.changes[1].rows.len(),
        2,
        "an edited list and an added note"
    );
    service.stop();
}

#[test]
fn a_document_that_is_as_approved_has_nothing_to_review() {
    let fx = Fx::new(&[("a.md", V0)]);
    let service = fx.service();
    tell(&service, &[doc("a.md", Some(V0), Some(V0))]);
    let view = service.view("a.md").expect("view");
    assert_eq!(view.status, ViewStatus::Unchanged);
    assert_eq!(view.hash, Some(hex(V0)));
    assert_eq!(view.changes.len(), 0);
    service.stop();
}

#[test]
fn blank_lines_alone_are_not_a_change_to_review() {
    let fx = Fx::new(&[("a.md", V0)]);
    let service = fx.service();
    tell(&service, &[doc("a.md", Some(V0), Some(V0))]);
    let spaced = V0.replace("\n\n", "\n\n\n");
    fx.write("a.md", &spaced);
    tell(&service, &[doc("a.md", Some(&spaced), Some(V0))]);
    assert_eq!(
        service.view("a.md").expect("view").status,
        ViewStatus::Unchanged
    );
    service.stop();
}

#[test]
fn a_new_document_is_all_addition() {
    let fx = Fx::new(&[("a.md", V0), ("new.md", "# New\n\nText.\n")]);
    let service = fx.service();
    tell(
        &service,
        &[
            doc("a.md", Some(V0), Some(V0)),
            doc("new.md", Some("# New\n\nText.\n"), None),
        ],
    );
    assert_eq!(service.entries(), [entry("new.md", Status::New)]);
    let view = service.view("new.md").expect("view");
    assert_eq!(view.status, ViewStatus::New);
    assert_eq!(view.reviewed_hash, None);
    assert_eq!(view.changes.len(), 1);
    assert_eq!(view.changes[0].kind, diff::ChangeKind::Added);
    service.stop();
}

#[test]
fn a_deleted_document_is_all_removal_and_can_be_put_back() {
    let fx = Fx::new(&[("a.md", V0), ("b.md", "# B\n")]);
    let service = fx.service();
    tell(
        &service,
        &[
            doc("a.md", Some(V0), Some(V0)),
            doc("b.md", Some("# B\n"), Some("# B\n")),
        ],
    );
    fx.remove("a.md");
    tell(&service, &[doc("b.md", Some("# B\n"), Some("# B\n"))]);
    assert_eq!(service.entries(), [entry("a.md", Status::Deleted)]);
    let view = service.view("a.md").expect("view");
    assert_eq!(view.status, ViewStatus::Deleted);
    assert!(view.restorable);
    assert_eq!(view.hash, None);
    assert_eq!(view.changes[0].kind, diff::ChangeKind::Removed);
    service.stop();
}

#[test]
fn a_deleted_document_without_a_kept_text_cannot_be_put_back() {
    let fx = Fx::new(&[]);
    // approved at a version nobody has the text of
    fs::write(
        fx.paths().state,
        format!(
            r#"{{ "version": 1, "documents": {{ "gone.md": {{ "hash": "{}" }} }} }}"#,
            hex("lost\n")
        ),
    )
    .expect("state");
    let service = fx.service();
    tell(&service, &[]);
    let view = service.view("gone.md").expect("view");
    assert_eq!(view.status, ViewStatus::Deleted);
    assert!(!view.restorable);
    assert_eq!(view.changes.len(), 0);
    let basis = basis(None, Some("lost\n"));
    assert!(matches!(
        service.restore("gone.md", &basis),
        Err(ReviewError::NoBaseline(_))
    ));
    service.stop();
}

#[test]
fn a_document_over_the_size_for_a_copy_is_only_ever_changed() {
    let big = "x\n\n".repeat(MAX_COPY_BYTES / 3 + 10);
    let fx = Fx::new(&[("big.md", &big)]);
    let service = fx.service();
    tell(&service, &[doc("big.md", Some(&big), Some(&big))]);
    assert_eq!(fx.copies(), HashSet::new());
    let more = format!("{big}more\n");
    fx.write("big.md", &more);
    tell(&service, &[doc("big.md", Some(&more), Some(&big))]);
    let view = service.view("big.md").expect("view");
    assert_eq!(view.status, ViewStatus::TooLarge);
    assert_eq!(view.hash, Some(hex(&more)));
    assert_eq!(view.changes.len(), 0);
    // keeping it all clears it, with no copy
    let view = service
        .keep_all("big.md", &basis(Some(&more), Some(&big)))
        .expect("keep all");
    assert_eq!(view.status, ViewStatus::TooLarge);
    assert_eq!(service.entries(), Vec::<Entry>::new());
    service.stop();
}

#[test]
fn a_file_that_is_not_utf8_cannot_be_compared() {
    let fx = Fx::new(&[("a.md", V0)]);
    let service = fx.service();
    tell(&service, &[doc("a.md", Some(V0), Some(V0))]);
    let latin = [b'#', b' ', 0xE9, b'\n'];
    fs::write(fx.project.path().join("a.md"), latin).expect("write");
    service.changed(
        Vec::new(),
        true,
        vec![DocState {
            path: "a.md".to_owned(),
            current: Some(hash_bytes(&latin)),
            read: Some(h(V0)),
        }],
    );
    service.flush();
    assert_eq!(
        service.view("a.md").expect("view").status,
        ViewStatus::NotText
    );
    service.stop();
}

#[test]
fn paths_are_checked_like_every_other_command_does() {
    let fx = Fx::new(&[("a.md", V0), ("notes.txt", "t")]);
    let service = fx.service();
    assert!(matches!(
        service.view("../outside.md"),
        Err(ReviewError::Vault(VaultError::Escapes(_)))
    ));
    assert!(matches!(
        service.view("/etc/passwd.md"),
        Err(ReviewError::Vault(VaultError::Invalid(_)))
    ));
    assert!(matches!(
        service.view("notes.txt"),
        Err(ReviewError::Edit(EditError::NotMarkdown(_)))
    ));
    let basis = basis(None, None);
    assert!(service.keep_all("../x.md", &basis).is_err());
    assert!(service.revert_all("../x.md", &basis).is_err());
    assert!(service.restore("../x.md", &basis).is_err());
    assert!(service.release("../x.md").is_err());
    service.stop();
}

#[test]
fn a_review_that_is_off_says_so() {
    let fx = Fx::new(&[("a.md", V0)]);
    let service = ReviewService::off(fx.vault(), Arc::clone(&fx.own));
    assert!(matches!(service.view("a.md"), Err(ReviewError::Off)));
    assert!(matches!(
        service.keep_all("a.md", &Basis::default()),
        Err(ReviewError::Off)
    ));
    assert_eq!(service.entries(), Vec::<Entry>::new());
    assert!(service.flush_within(Duration::ZERO));
    service.stop();
}

// ---- keeping ----

#[test]
fn keeping_a_change_makes_it_part_of_the_approved_version() {
    let fx = Fx::new(&[]);
    let service = fx.agent_changed_a();
    let view = service
        .keep("a.md", &basis(Some(V1), Some(V0)), 0)
        .expect("keep");
    assert_eq!(view.status, ViewStatus::Changes);
    assert_eq!(view.changes.len(), 1, "the other is still to review");
    let kept = "# Plan\n\nThe launch is planned for July and the team is ready.\n\nBudget stays the same.\n\n- one\n- two\n";
    assert_eq!(view.reviewed_hash, Some(hex(kept)));
    assert_eq!(fx.read("a.md"), V1, "the file is not touched");
    assert!(fx.copies().contains(&h(kept)));
    assert_eq!(service.entries(), [entry("a.md", Status::Changed)]);

    let view = service
        .keep("a.md", &basis(Some(V1), Some(kept)), 0)
        .expect("keep");
    assert_eq!(view.status, ViewStatus::Unchanged);
    assert_eq!(service.entries(), Vec::<Entry>::new());
    assert_eq!(fx.last_heard(), Some(Vec::new()));
    service.flush();
    assert_eq!(fx.saved().expect("saved")["a.md"], hex(V1));
    service.stop();
}

#[test]
fn keeping_all_makes_the_document_as_it_is_the_approved_version() {
    let fx = Fx::new(&[]);
    let service = fx.agent_changed_a();
    let view = service
        .keep_all("a.md", &basis(Some(V1), Some(V0)))
        .expect("keep all");
    assert_eq!(view.status, ViewStatus::Unchanged);
    assert_eq!(view.reviewed_hash, Some(hex(V1)));
    assert_eq!(fx.read("a.md"), V1);
    assert_eq!(service.entries(), Vec::<Entry>::new());
    service.stop();
}

#[test]
fn keeping_after_the_file_or_the_approved_version_moved_on_is_refused() {
    let fx = Fx::new(&[]);
    let service = fx.agent_changed_a();
    // the agent wrote again after the review was drawn
    let v2 = format!("{V1}\nAnd one more.\n");
    fx.write("a.md", &v2);
    tell(&service, &[doc("a.md", Some(&v2), Some(V0))]);
    let stale_file = basis(Some(V1), Some(V0));
    assert!(matches!(
        service.keep("a.md", &stale_file, 0),
        Err(ReviewError::Stale(_))
    ));
    assert!(matches!(
        service.keep_all("a.md", &stale_file),
        Err(ReviewError::Stale(_))
    ));
    // or the approved version moved (a keep from another window)
    let stale_reviewed = basis(Some(&v2), Some("# Something else\n"));
    assert!(matches!(
        service.keep("a.md", &stale_reviewed, 0),
        Err(ReviewError::Stale(_))
    ));
    // and a change that is not there
    let now = basis(Some(&v2), Some(V0));
    assert!(matches!(
        service.keep("a.md", &now, 99),
        Err(ReviewError::Stale(_))
    ));
    service.stop();
}

#[test]
fn keeping_a_new_document_approves_it() {
    let fx = Fx::new(&[("new.md", "# New\n")]);
    let service = fx.service();
    tell(&service, &[doc("new.md", Some("# New\n"), None)]);
    let view = service
        .keep_all("new.md", &basis(Some("# New\n"), None))
        .expect("keep all");
    assert_eq!(view.status, ViewStatus::Unchanged);
    assert_eq!(service.entries(), Vec::<Entry>::new());
    service.flush();
    assert_eq!(fx.saved().expect("saved")["new.md"], hex("# New\n"));
    service.stop();
}

#[test]
fn keeping_a_deletion_forgets_the_document() {
    let fx = Fx::new(&[("a.md", V0)]);
    let service = fx.service();
    tell(&service, &[doc("a.md", Some(V0), Some(V0))]);
    fx.remove("a.md");
    tell(&service, &[]);
    assert_eq!(service.entries(), [entry("a.md", Status::Deleted)]);
    let view = service
        .keep_all("a.md", &basis(None, Some(V0)))
        .expect("keep");
    assert_eq!(view.status, ViewStatus::Unchanged);
    assert_eq!(service.entries(), Vec::<Entry>::new());
    service.flush();
    assert!(!fx.saved().expect("saved").contains_key("a.md"));
    service.stop();
}

// ---- reverting ----

#[test]
fn reverting_a_change_writes_the_old_text_back_and_leaves_the_approved_version_alone() {
    let fx = Fx::new(&[]);
    let service = fx.agent_changed_a();
    let reverted = service
        .revert("a.md", &basis(Some(V1), Some(V0)), 1)
        .expect("revert");
    let expected = "# Plan\n\nThe launch is planned for July and the team is ready.\n\nBudget stays the same.\n\n- one\n- two\n";
    assert_eq!(fx.read("a.md"), expected);
    assert_eq!(
        reverted,
        Reverted::Wrote {
            old_hash: hex(V1),
            new_hash: hex(expected)
        }
    );
    // the approved version is what it was: the difference just lost a change
    let view = service.view("a.md").expect("view");
    assert_eq!(view.reviewed_hash, Some(hex(V0)));
    assert_eq!(view.hash, Some(hex(expected)));
    assert_eq!(view.changes.len(), 1);
    service.stop();
}

#[test]
fn a_revert_is_the_apps_own_write_and_the_watcher_does_not_report_it_as_news() {
    let fx = Fx::new(&[]);
    let service = fx.agent_changed_a();
    let Reverted::Wrote { new_hash, .. } = service
        .revert("a.md", &basis(Some(V1), Some(V0)), 0)
        .expect("revert")
    else {
        panic!("written");
    };
    let sigs = SigStore::new();
    sigs.set(
        key("a.md"),
        "a.md",
        FileSig {
            len: 1,
            mtime_ms: 1,
            hash: crate::unread::hash_from_hex(&new_hash).expect("hash"),
        },
    );
    let mut changes = vec![FsChange {
        kind: ChangeKind::Modified,
        path: "a.md".to_owned(),
        from: None,
        at_ms: 1,
    }];
    fx.own.strip(&mut changes, &sigs);
    assert_eq!(changes, Vec::<FsChange>::new());
    service.stop();
}

#[test]
fn a_revert_never_overwrites_a_file_that_changed_since_the_review_was_drawn() {
    let fx = Fx::new(&[]);
    let service = fx.agent_changed_a();
    let v2 = format!("{V1}\nWritten by the agent a moment ago.\n");
    fx.write("a.md", &v2);
    let result = service.revert("a.md", &basis(Some(V1), Some(V0)), 0);
    assert!(matches!(result, Err(ReviewError::Stale(_))), "{result:?}");
    let result = service.revert_all("a.md", &basis(Some(V1), Some(V0)));
    assert!(matches!(result, Err(ReviewError::Stale(_))), "{result:?}");
    assert_eq!(fx.read("a.md"), v2, "untouched");
    service.stop();
}

#[test]
fn reverting_after_the_approved_version_moved_on_is_refused() {
    let fx = Fx::new(&[]);
    let service = fx.agent_changed_a();
    let result = service.revert("a.md", &basis(Some(V1), Some("# Other\n")), 0);
    assert!(matches!(result, Err(ReviewError::Stale(_))));
    let result = service.revert("a.md", &basis(Some(V1), Some(V0)), 7);
    assert!(
        matches!(result, Err(ReviewError::Stale(_))),
        "no such change"
    );
    assert_eq!(fx.read("a.md"), V1);
    service.stop();
}

#[test]
fn reverting_all_writes_the_approved_text_exactly() {
    let fx = Fx::new(&[]);
    let service = fx.agent_changed_a();
    let reverted = service
        .revert_all("a.md", &basis(Some(V1), Some(V0)))
        .expect("revert all");
    assert_eq!(fx.read("a.md"), V0);
    assert_eq!(
        reverted,
        Reverted::Wrote {
            old_hash: hex(V1),
            new_hash: hex(V0)
        }
    );
    service.stop();
}

#[test]
fn a_file_the_agent_rewrote_from_scratch_is_given_back_whole() {
    let fx = Fx::new(&[("a.md", "\u{FEFF}# Old\r\n\r\nalpha beta gamma\r\n")]);
    let old = "\u{FEFF}# Old\r\n\r\nalpha beta gamma\r\n";
    let service = fx.service();
    tell(&service, &[doc("a.md", Some(old), Some(old))]);
    let rewritten = "Something entirely different.\n\nNo words in common.\n";
    fx.write("a.md", rewritten);
    tell(&service, &[doc("a.md", Some(rewritten), Some(old))]);
    let view = service.view("a.md").expect("view");
    assert_eq!(view.changes.len(), 1);
    service
        .revert("a.md", &basis(Some(rewritten), Some(old)), 0)
        .expect("revert");
    assert_eq!(
        fx.read("a.md"),
        old,
        "byte for byte, the BOM and line endings too"
    );
    service.stop();
}

#[test]
fn reverting_a_new_document_is_left_to_the_caller_and_touches_nothing() {
    let fx = Fx::new(&[("new.md", "# New\n")]);
    let service = fx.service();
    tell(&service, &[doc("new.md", Some("# New\n"), None)]);
    let reverted = service
        .revert("new.md", &basis(Some("# New\n"), None), 0)
        .expect("revert");
    assert_eq!(reverted, Reverted::NewDocument);
    assert_eq!(fx.read("new.md"), "# New\n");
    let reverted = service
        .revert_all("new.md", &basis(Some("# New\n"), None))
        .expect("revert");
    assert_eq!(reverted, Reverted::NewDocument);
    service.stop();
}

#[test]
fn a_deleted_document_is_put_back_exactly_as_it_was_approved() {
    let text = "\u{FEFF}# Docs\r\n\r\nText.\r\n";
    let fx = Fx::new(&[("docs/deep/a.md", text)]);
    let service = fx.service();
    tell(&service, &[doc("docs/deep/a.md", Some(text), Some(text))]);
    fs::remove_dir_all(fx.project.path().join("docs")).expect("remove");
    tell(&service, &[]);
    let restored = service
        .restore("docs/deep/a.md", &basis(None, Some(text)))
        .expect("restore");
    assert_eq!(restored, Reverted::Restored { hash: hex(text) });
    assert_eq!(
        fs::read(fx.project.path().join("docs/deep/a.md")).expect("read"),
        text.as_bytes()
    );
    service.stop();
}

#[test]
fn restoring_a_document_that_is_there_again_is_refused() {
    let fx = Fx::new(&[("a.md", V0)]);
    let service = fx.service();
    tell(&service, &[doc("a.md", Some(V0), Some(V0))]);
    // the basis says it is gone, but it is not
    let result = service.restore("a.md", &basis(None, Some(V0)));
    assert!(matches!(result, Err(ReviewError::Stale(_))), "{result:?}");
    service.stop();
}

// ---- a review that is open ----

#[test]
fn opening_a_changed_document_keeps_its_changes_to_review() {
    let fx = Fx::new(&[]);
    let service = fx.agent_changed_a();
    fx.read_by_person(&service, "a.md", &[doc("a.md", Some(V1), Some(V1))]);
    let view = service.view("a.md").expect("view");
    assert_eq!(
        view.status,
        ViewStatus::Changes,
        "read, and still reviewable"
    );
    assert_eq!(view.reviewed_hash, Some(hex(V0)));
    assert_eq!(view.changes.len(), 2);
    assert_eq!(service.entries(), [entry("a.md", Status::Changed)]);
    service.stop();
}

#[test]
fn keeping_and_reverting_work_on_the_version_held_for_the_review() {
    let fx = Fx::new(&[]);
    let service = fx.agent_changed_a();
    fx.read_by_person(&service, "a.md", &[doc("a.md", Some(V1), Some(V1))]);
    let view = service
        .keep("a.md", &basis(Some(V1), Some(V0)), 0)
        .expect("keep");
    assert_eq!(view.changes.len(), 1);
    let kept = "# Plan\n\nThe launch is planned for July and the team is ready.\n\nBudget stays the same.\n\n- one\n- two\n";
    assert_eq!(
        view.reviewed_hash,
        Some(hex(kept)),
        "the held version moved"
    );
    // the revert of the rest is the app's own write: it stays read
    let Reverted::Wrote { old_hash, new_hash } = service
        .revert("a.md", &basis(Some(V1), Some(kept)), 0)
        .expect("revert")
    else {
        panic!("written");
    };
    assert_eq!((old_hash, new_hash), (hex(V1), hex(kept)));
    tell(&service, &[doc("a.md", Some(kept), Some(kept))]);
    let view = service.view("a.md").expect("view");
    assert_eq!(
        view.status,
        ViewStatus::Unchanged,
        "nothing is left, the review is over"
    );
    assert_eq!(service.entries(), Vec::<Entry>::new());
    service.stop();
}

#[test]
fn closing_the_review_lets_the_held_version_go() {
    let fx = Fx::new(&[]);
    let service = fx.agent_changed_a();
    fx.read_by_person(&service, "a.md", &[doc("a.md", Some(V1), Some(V1))]);
    service.release("a.md").expect("release");
    let view = service.view("a.md").expect("view");
    assert_eq!(view.status, ViewStatus::Unchanged);
    assert_eq!(view.reviewed_hash, Some(hex(V1)));
    assert_eq!(service.entries(), Vec::<Entry>::new());
    service.age_copies();
    service.flush();
    assert_eq!(fx.copies(), HashSet::from([h(V1)]), "and so does its copy");
    service.stop();
}

#[test]
fn a_bulk_reading_holds_nothing() {
    let fx = Fx::new(&[]);
    let service = fx.agent_changed_a();
    // mark all read: nobody opened the document
    tell(&service, &[doc("a.md", Some(V1), Some(V1))]);
    assert_eq!(
        service.view("a.md").expect("view").status,
        ViewStatus::Unchanged
    );
    service.stop();
}

// ---- moves and events ----

#[test]
fn a_moved_document_keeps_its_review() {
    let fx = Fx::new(&[]);
    let service = fx.agent_changed_a();
    fs::rename(
        fx.project.path().join("a.md"),
        fx.project.path().join("b.md"),
    )
    .expect("rename");
    service.changed(
        vec![("a.md".to_owned(), "b.md".to_owned())],
        true,
        vec![doc("b.md", Some(V1), Some(V0))],
    );
    service.flush();
    assert_eq!(service.entries(), [entry("b.md", Status::Changed)]);
    let view = service.view("b.md").expect("view");
    assert_eq!(view.status, ViewStatus::Changes);
    assert_eq!(view.changes.len(), 2);
    service.flush();
    assert_eq!(
        fx.saved().expect("saved").keys().collect::<Vec<_>>(),
        ["b.md"]
    );
    service.stop();
}

#[test]
fn the_listener_hears_the_documents_each_time_they_change_and_not_otherwise() {
    let fx = Fx::new(&[]);
    let service = fx.agent_changed_a();
    let heard = lock(&fx.heard).len();
    tell(&service, &[doc("a.md", Some(V1), Some(V0))]);
    assert_eq!(
        lock(&fx.heard).len(),
        heard,
        "the same documents are not told again"
    );
    service
        .revert_all("a.md", &basis(Some(V1), Some(V0)))
        .expect("revert");
    tell(&service, &[doc("a.md", Some(V0), Some(V0))]);
    assert_eq!(fx.last_heard(), Some(Vec::new()));
    service.stop();
}

// ---- the files move ----

#[test]
fn when_the_project_gets_a_folder_for_its_state_the_copies_follow() {
    let fx = Fx::new(&[]);
    let service = fx.agent_changed_a();
    let elsewhere = tempfile::tempdir().expect("tempdir");
    let new_paths = Paths {
        state: elsewhere.path().join(".trailmark/local/reviewed.json"),
        copies: elsewhere.path().join(".trailmark/local/reviewed"),
    };
    service.relocate(new_paths.clone());
    service.flush();
    assert_eq!(
        CopyStore::new(new_paths.copies.clone()).hashes(),
        HashSet::from([h(V0)])
    );
    assert!(new_paths.state.is_file());
    assert_eq!(fx.copies(), HashSet::new(), "and the old place is empty");
    // reading goes on there
    fx.read_by_person(&service, "a.md", &[doc("a.md", Some(V1), Some(V1))]);
    assert!(CopyStore::new(new_paths.copies).hashes().contains(&h(V1)));
    service.stop();
}

#[test]
fn moving_the_files_to_where_they_already_are_deletes_nothing() {
    let fx = Fx::new(&[]);
    let service = fx.agent_changed_a();
    service.relocate(fx.paths());
    service.flush();
    assert!(fx.copies().contains(&h(V0)));
    assert!(fx.saved().is_some());
    service.stop();
}

// ---- with the unread markers ----

#[test]
fn followed_from_the_unread_markers_a_review_lives_through_reading_and_reverting() {
    let fx = Fx::new(&[("a.md", V0), ("b.md", "# B\n")]);
    let review = Arc::new(fx.service());
    let vault = fx.vault();
    let filter = Arc::new(PathFilter::new(&[(vault.root().to_path_buf(), None)], &[]));
    let unread = UnreadService::open(
        vault,
        filter,
        fx.state.path().join("read.json"),
        NEVER,
        Arc::new(|_: &[String]| {}),
        Some(Arc::clone(&review) as Arc<dyn ReadObserver>),
    );
    unread.flush();
    review.flush();
    assert_eq!(
        fx.copies(),
        HashSet::from([h(V0), h("# B\n")]),
        "the baseline"
    );

    // an agent edits a.md while the person is elsewhere
    fx.write("a.md", V1);
    unread.apply(&[FsChange {
        kind: ChangeKind::Modified,
        path: "a.md".to_owned(),
        from: None,
        at_ms: 0,
    }]);
    unread.flush();
    review.flush();
    assert_eq!(unread.unread(), ["a.md"]);
    assert_eq!(review.entries(), [entry("a.md", Status::Changed)]);

    // the person opens it: it counts as read, and the changes are still there
    unread.mark_read("a.md").expect("read");
    review.flush();
    assert_eq!(unread.unread(), Vec::<String>::new());
    let view = review.view("a.md").expect("view");
    assert_eq!(view.status, ViewStatus::Changes);
    assert_eq!(view.changes.len(), 2);

    // reverting one change is the app's own doing: the document stays read
    let Reverted::Wrote { old_hash, new_hash } = review
        .revert("a.md", &basis(Some(V1), Some(V0)), 0)
        .expect("revert")
    else {
        panic!("written");
    };
    unread.rewritten("a.md", &old_hash, &new_hash);
    unread.flush();
    review.flush();
    assert_eq!(unread.unread(), Vec::<String>::new());
    let view = review.view("a.md").expect("view");
    assert_eq!(view.status, ViewStatus::Changes);
    assert_eq!(
        view.reviewed_hash,
        Some(hex(V0)),
        "still compared with what was read first"
    );
    assert_eq!(view.changes.len(), 1);

    // closing the review approves what is left
    review.release("a.md").expect("release");
    review.flush();
    assert_eq!(
        review.view("a.md").expect("view").status,
        ViewStatus::Unchanged
    );
    assert_eq!(review.entries(), Vec::<Entry>::new());
    unread.stop();
    review.stop();
}

// ---- documents that are gone ----

/// `reviewed.json` as an earlier run could have left it: `docs` are `(path, approved text, when it was seen to be gone)`.
fn write_state(fx: &Fx, docs: &[(&str, &str, Option<u64>)]) {
    let documents: serde_json::Map<String, serde_json::Value> = docs
        .iter()
        .map(|(path, text, gone)| {
            let mut entry = serde_json::json!({ "hash": hex(text) });
            if let Some(at) = gone {
                entry["deletedAt"] = serde_json::json!(at);
            }
            ((*path).to_owned(), entry)
        })
        .collect();
    fs::create_dir_all(fx.state.path()).expect("mkdir");
    fs::write(
        fx.paths().state,
        serde_json::json!({ "version": 1, "documents": documents }).to_string(),
    )
    .expect("state");
}

#[test]
fn a_document_seen_to_be_gone_is_dated_in_the_saved_file_and_listed_as_deleted() {
    let fx = Fx::new(&[("a.md", V0)]);
    let service = fx.service();
    tell(&service, &[doc("a.md", Some(V0), Some(V0))]);
    fx.remove("a.md");
    let before = now_ms();
    tell(&service, &[]);
    assert_eq!(service.entries(), [entry("a.md", Status::Deleted)]);
    service.flush();
    let file: serde_json::Value =
        serde_json::from_slice(&fs::read(fx.paths().state).expect("read")).expect("json");
    let at = file["documents"]["a.md"]["deletedAt"]
        .as_u64()
        .expect("dated");
    assert!(at >= before && at <= now_ms(), "{at}");
    service.stop();
}

#[test]
fn a_document_that_was_gone_for_over_thirty_days_is_forgotten_when_the_project_is_looked_at() {
    let fx = Fx::new(&[]);
    // long ago: 1970
    write_state(
        &fx,
        &[("old.md", "old text\n", Some(1)), ("recent.md", V0, None)],
    );
    let service = fx.service();
    service.keep_copy(b"old text\n").expect("copy");
    tell(&service, &[]);
    // recent.md is not on the disk either, and gets its date now
    assert_eq!(service.entries(), [entry("recent.md", Status::Deleted)]);
    service.age_copies();
    service.flush();
    let saved = fx.saved().expect("saved");
    assert_eq!(saved.keys().collect::<Vec<_>>(), ["recent.md"]);
    assert!(!fx.copies().contains(&h("old text\n")), "and its copy goes");
    service.stop();
}

#[test]
fn entries_that_are_not_paths_of_the_project_are_not_listed_as_deleted() {
    let fx = Fx::new(&[]);
    write_state(
        &fx,
        &[
            ("../outside.md", V0, None),
            ("C:/elsewhere.md", V0, None),
            ("fine.md", V0, None),
        ],
    );
    let service = fx.service();
    tell(&service, &[]);
    assert_eq!(service.entries(), [entry("fine.md", Status::Deleted)]);
    service.stop();
}

#[test]
fn a_copy_just_made_but_not_pointed_to_yet_follows_the_others_and_nothing_is_left_behind() {
    let fx = Fx::new(&[]);
    let service = fx.agent_changed_a();
    // a Keep that is under way: its copy exists, no version points to it yet
    service.keep_copy(b"# In flight\n").expect("copy");
    let elsewhere = tempfile::tempdir().expect("tempdir");
    let new_paths = Paths {
        state: elsewhere.path().join(".trailmark/local/reviewed.json"),
        copies: elsewhere.path().join(".trailmark/local/reviewed"),
    };
    service.relocate(new_paths.clone());
    service.flush();
    assert_eq!(
        CopyStore::new(new_paths.copies).hashes(),
        HashSet::from([h(V0), h("# In flight\n")])
    );
    assert_eq!(
        fx.copies(),
        HashSet::new(),
        "the old place keeps nothing, the copies nothing pointed to included"
    );
    service.stop();
}

#[test]
fn copies_that_could_not_be_moved_are_not_deleted_from_where_they_were() {
    let fx = Fx::new(&[]);
    let service = fx.agent_changed_a();
    assert!(fx.copies().contains(&h(V0)));
    // the new place cannot be made: a file is where its folder would be
    let blocker = tempfile::tempdir().expect("tempdir");
    fs::write(blocker.path().join("local"), "a file").expect("file");
    service.relocate(Paths {
        state: blocker.path().join("local/reviewed.json"),
        copies: blocker.path().join("local/reviewed"),
    });
    service.flush();
    assert!(
        fx.copies().contains(&h(V0)),
        "the baseline is still where it was"
    );
    service.stop();
}

#[test]
fn moving_the_files_does_not_hold_up_what_the_commands_do() {
    let fx = Fx::new(&[]);
    let service = fx.agent_changed_a();
    // many copies to carry over
    for i in 0..300 {
        service
            .keep_copy(format!("# copy {i}\n").as_bytes())
            .expect("copy");
    }
    let elsewhere = tempfile::tempdir().expect("tempdir");
    service.relocate(Paths {
        state: elsewhere.path().join("local/reviewed.json"),
        copies: elsewhere.path().join("local/reviewed"),
    });
    // while the thread carries them, the lock is free: a view is answered at once and correctly
    let started = Instant::now();
    let view = service.view("a.md").expect("view");
    assert_eq!(view.status, ViewStatus::Changes);
    assert!(started.elapsed() < Duration::from_secs(5));
    service.flush();
    service.stop();
}

// ---- what is remembered as lost ----

#[test]
fn a_version_too_large_for_a_copy_is_remembered_as_such() {
    let big = "x\n\n".repeat(MAX_COPY_BYTES / 3 + 10);
    let fx = Fx::new(&[("big.md", &big)]);
    let service = fx.service();
    tell(&service, &[doc("big.md", Some(&big), Some(&big))]);
    assert!(lock(&service.shared.inner).huge.contains(&h(&big)));
    assert_eq!(fx.copies(), HashSet::new());
    tell(&service, &[doc("big.md", Some(&big), Some(&big))]);
    assert!(lock(&service.shared.inner).huge.contains(&h(&big)));
    // and when it is another version by now, the memory of it goes
    let small = "# Small now\n";
    fx.write("big.md", small);
    tell(&service, &[doc("big.md", Some(small), Some(small))]);
    assert!(lock(&service.shared.inner).huge.is_empty());
    service.stop();
}

#[test]
fn versions_nobody_wants_any_more_are_not_remembered_as_lost() {
    let fx = Fx::new(&[("a.md", V1)]);
    let commits = Commits(HashMap::from([(
        "a.md".to_owned(),
        b"# Some other version\n".to_vec(),
    )]));
    let service = fx.service_with(Some(Arc::new(commits)));
    tell(&service, &[doc("a.md", Some(V1), Some(V0))]);
    assert_eq!(lock(&service.shared.inner).missing.len(), 1);
    // everything is marked read: V1 is the approved version, and V0 is not wanted any more
    tell(&service, &[doc("a.md", Some(V1), Some(V1))]);
    assert!(lock(&service.shared.inner).missing.is_empty());
    service.stop();
}

// ---- a pile of looks at the project ----

fn look(moves: &[(&str, &str)], looked: bool, docs: &[DocState]) -> Look {
    Look {
        moves: moves
            .iter()
            .map(|(from, to)| ((*from).to_owned(), (*to).to_owned()))
            .collect(),
        looked,
        docs: docs.to_vec(),
    }
}

#[test]
fn looks_that_piled_up_are_taken_as_one_with_the_moves_in_order_and_the_newest_documents() {
    let (tx, rx) = mpsc::channel();
    let old = [doc("old.md", Some("1"), Some("1"))];
    let newer = [doc("new.md", Some("2"), Some("2"))];
    let newest = [doc("z.md", Some("3"), Some("3"))];
    tx.send(Msg::Changed(look(&[("a.md", "b.md")], false, &old)))
        .expect("send");
    tx.send(Msg::Changed(look(&[("b.md", "c.md")], true, &newer)))
        .expect("send");
    tx.send(Msg::Changed(look(&[], false, &newest)))
        .expect("send");
    tx.send(Msg::Opening("c.md".to_owned(), 7)).expect("send");
    tx.send(Msg::Changed(look(&[("x.md", "y.md")], false, &old)))
        .expect("send");

    let first = look(&[("start.md", "a.md")], false, &old);
    let (gathered, rest) = gather(&rx, first);
    assert_eq!(
        gathered.moves,
        [
            ("start.md".to_owned(), "a.md".to_owned()),
            ("a.md".to_owned(), "b.md".to_owned()),
            ("b.md".to_owned(), "c.md".to_owned())
        ]
    );
    assert!(gathered.looked, "the disk was looked at in one of them");
    assert_eq!(gathered.docs, newest);
    // what came after another kind of message is not taken ahead of it
    assert!(matches!(rest, Some(Msg::Opening(path, 7)) if path == "c.md"));
    assert!(matches!(rx.try_recv(), Ok(Msg::Changed(_))), "still queued");
}

#[test]
fn with_nothing_queued_behind_it_a_look_is_taken_as_it_is() {
    let (_tx, rx) = mpsc::channel();
    let only = [doc("a.md", Some("1"), Some("1"))];
    let (gathered, rest) = gather(&rx, look(&[("a.md", "b.md")], true, &only));
    assert_eq!(gathered.moves.len(), 1);
    assert_eq!(gathered.docs, only);
    assert!(rest.is_none());
}
