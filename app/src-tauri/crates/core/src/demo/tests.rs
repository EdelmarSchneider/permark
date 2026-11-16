use std::fs;
use std::path::{Path, PathBuf};

use super::*;
use crate::error::ErrorKind;
use crate::state::AppDirs;

struct Fx {
    base: tempfile::TempDir,
    store: StateStore,
    bundled: PathBuf,
    documents: PathBuf,
}

impl Fx {
    fn places(&self) -> DemoPlaces {
        DemoPlaces {
            bundled: Some(self.bundled.clone()),
            target: Some(self.target()),
        }
    }

    fn target(&self) -> PathBuf {
        self.documents.join(FOLDER)
    }
}

/// The pages as shipped (a CRLF file with a BOM among them) and an empty Documents folder.
fn fx() -> Fx {
    let base = tempfile::tempdir().unwrap();
    let bundled = base.path().join("resources").join(RESOURCE);
    fs::create_dir_all(bundled.join("station")).unwrap();
    fs::write(bundled.join("README.md"), "# Demo\n").unwrap();
    fs::write(bundled.join("crlf.md"), b"\xEF\xBB\xBF# One\r\ntwo\r\n").unwrap();
    fs::write(bundled.join(".aiignore"), "station/deploy.env\n").unwrap();
    fs::write(bundled.join("station/deploy.env"), "KEY=1\n").unwrap();
    let documents = base.path().join("Documents");
    fs::create_dir_all(&documents).unwrap();
    Fx {
        store: StateStore::app(&AppDirs::under(&base.path().join("state"))),
        bundled,
        documents,
        base,
    }
}

fn names(dir: &Path) -> Vec<String> {
    let mut out: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    out.sort();
    out
}

#[test]
fn a_fresh_install_copies_the_pages_into_documents_byte_for_byte() {
    let f = fx();
    assert_eq!(offer(&f.store, &f.places()), DemoOffer::Install);
    let placed = install(&f.store, &f.places()).unwrap();
    assert_eq!(placed, f.target());
    assert_eq!(
        fs::read(f.target().join("crlf.md")).unwrap(),
        b"\xEF\xBB\xBF# One\r\ntwo\r\n"
    );
    assert_eq!(
        fs::read_to_string(f.target().join(".aiignore")).unwrap(),
        "station/deploy.env\n"
    );
    assert_eq!(
        fs::read_to_string(f.target().join("station/deploy.env")).unwrap(),
        "KEY=1\n"
    );
    // nothing else in Documents: no temp left beside the folder
    assert_eq!(names(&f.documents), [FOLDER]);
    // used: the offer is not shown again
    assert_eq!(offer(&f.store, &f.places()), DemoOffer::Hidden);
}

#[test]
fn a_folder_already_in_documents_is_opened_as_it_is_never_written_into() {
    let f = fx();
    fs::create_dir_all(f.target()).unwrap();
    fs::write(f.target().join("README.md"), "my own notes\r\n").unwrap();
    assert_eq!(offer(&f.store, &f.places()), DemoOffer::Open);
    let placed = install(&f.store, &f.places()).unwrap();
    assert_eq!(placed, f.target());
    assert_eq!(
        fs::read(f.target().join("README.md")).unwrap(),
        b"my own notes\r\n"
    );
    assert_eq!(names(&f.target()), ["README.md"]);
    assert_eq!(offer(&f.store, &f.places()), DemoOffer::Hidden);
}

#[test]
fn a_file_where_the_folder_would_go_is_left_alone_and_refused() {
    let f = fx();
    fs::write(f.target(), "a file").unwrap();
    assert_eq!(offer(&f.store, &f.places()), DemoOffer::Hidden);
    let error = install(&f.store, &f.places()).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Exists);
    assert_eq!(fs::read_to_string(f.target()).unwrap(), "a file");
}

#[test]
fn a_failed_copy_leaves_nothing_in_documents_and_keeps_the_offer() {
    let f = fx();
    let places = DemoPlaces {
        bundled: Some(f.bundled.join("not-there")),
        target: Some(f.target()),
    };
    assert_eq!(offer(&f.store, &places), DemoOffer::Hidden);
    let error = install(&f.store, &places).unwrap_err();
    assert_eq!(error.kind, ErrorKind::NotFound);
    assert_eq!(names(&f.documents), Vec::<String>::new());
    // the pages themselves are fine: still offered
    assert_eq!(offer(&f.store, &f.places()), DemoOffer::Install);
}

#[test]
fn a_missing_documents_folder_is_made_and_no_place_at_all_hides_the_offer() {
    let f = fx();
    fs::remove_dir(&f.documents).unwrap();
    install(&f.store, &f.places()).unwrap();
    assert!(f.target().join("README.md").is_file());
    let nowhere = DemoPlaces {
        bundled: Some(f.bundled.clone()),
        target: None,
    };
    let g = fx();
    assert_eq!(offer(&g.store, &nowhere), DemoOffer::Hidden);
    assert_eq!(
        install(&g.store, &nowhere).unwrap_err().kind,
        ErrorKind::Unavailable
    );
}

#[test]
fn a_dismissed_offer_stays_dismissed() {
    let f = fx();
    dismiss(&f.store).unwrap();
    assert_eq!(offer(&f.store, &f.places()), DemoOffer::Hidden);
    assert_eq!(names(&f.documents), Vec::<String>::new());
    // the state survives a new start (read from the profile again)
    let again = StateStore::app(&AppDirs::under(&f.base.path().join("state")));
    assert_eq!(offer(&again, &f.places()), DemoOffer::Hidden);
}

#[test]
fn the_offer_serializes_as_the_frontend_reads_it() {
    assert_eq!(
        serde_json::to_string(&DemoOffer::Install).unwrap(),
        "\"install\""
    );
    assert_eq!(serde_json::to_string(&DemoOffer::Open).unwrap(), "\"open\"");
    assert_eq!(
        serde_json::to_string(&DemoOffer::Hidden).unwrap(),
        "\"hidden\""
    );
}

#[test]
fn the_pages_stay_available_after_the_offer_was_used_or_dismissed() {
    let f = fx();
    let fresh = status(&f.store, &f.places());
    assert_eq!(fresh.offer, DemoOffer::Install);
    assert!(fresh.available);
    dismiss(&f.store).unwrap();
    let dismissed = status(&f.store, &f.places());
    assert_eq!(dismissed.offer, DemoOffer::Hidden);
    assert!(dismissed.available);
    // used: the folder is in Documents now and opens as it is
    install(&f.store, &f.places()).unwrap();
    assert!(status(&f.store, &f.places()).available);
    assert_eq!(
        serde_json::to_string(&status(&f.store, &f.places())).unwrap(),
        r#"{"offer":"hidden","available":true}"#
    );
}

#[test]
fn the_pages_are_unavailable_without_a_place_or_anything_to_open() {
    let f = fx();
    let nowhere = DemoPlaces {
        bundled: Some(f.bundled.clone()),
        target: None,
    };
    assert!(!status(&f.store, &nowhere).available);
    let nothing_shipped = DemoPlaces {
        bundled: Some(f.base.path().join("missing")),
        target: Some(f.target()),
    };
    assert!(!status(&f.store, &nothing_shipped).available);
    // something other than a folder at the place: install refuses it, so nothing is offered
    fs::write(f.target(), "x").unwrap();
    assert!(!status(&f.store, &f.places()).available);
}
