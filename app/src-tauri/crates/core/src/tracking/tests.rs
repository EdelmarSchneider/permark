use std::convert::Infallible;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::*;
use crate::tree::Category;

const FP: &str = "model-a/1/384/";

/// mtime well outside the racy window.
fn old_time() -> SystemTime {
    SystemTime::now() - Duration::from_secs(3600)
}

fn write(dir: &Path, path: &str, bytes: &[u8], time: Option<SystemTime>) {
    let abs = dir.join(path);
    if let Some(parent) = abs.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&abs, bytes).unwrap();
    if let Some(time) = time {
        set_time(&abs, time);
    }
}

fn set_time(abs: &Path, time: SystemTime) {
    File::options()
        .write(true)
        .open(abs)
        .unwrap()
        .set_modified(time)
        .unwrap();
}

fn file_of(dir: &Path, path: &str) -> ScanFile {
    let abs: PathBuf = dir.join(path);
    let meta = fs::metadata(&abs).unwrap();
    let ms = meta
        .modified()
        .unwrap()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    ScanFile {
        path: path.to_owned(),
        category: Category::Markdown,
        abs: Some(abs),
        size: meta.len(),
        modified_ms: i64::try_from(ms).unwrap(),
    }
}

fn scan_of(dir: &Path, paths: &[&str], cut_off: bool) -> Scan {
    Scan {
        files: paths.iter().map(|p| file_of(dir, p)).collect(),
        unchecked: Vec::new(),
        cut_off,
    }
}

fn run(prev: Option<&Snapshot>, fp: &str, scan: Scan) -> Checked {
    check(prev, fp, || Ok::<_, Infallible>(scan)).unwrap()
}

fn owned(paths: &[&str]) -> Vec<String> {
    paths.iter().map(|p| (*p).to_owned()).collect()
}

#[test]
fn reports_changed_added_removed_exactly() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    for p in ["a.md", "b.md", "c.md", "sub/d.md"] {
        write(d, p, p.as_bytes(), Some(old_time()));
    }
    let first = run(
        None,
        FP,
        scan_of(d, &["a.md", "b.md", "c.md", "sub/d.md"], false),
    );
    assert!(first
        .next
        .files
        .values()
        .all(|e| !e.racy && e.hash.is_some()));

    write(d, "b.md", b"new bytes of b", Some(old_time()));
    fs::remove_file(d.join("c.md")).unwrap();
    write(d, "e.md", b"e", Some(old_time()));
    let second = run(
        Some(&first.next),
        FP,
        scan_of(d, &["a.md", "b.md", "sub/d.md", "e.md"], false),
    );
    assert_eq!(
        second.freshness,
        Freshness::Stale {
            changed: owned(&["b.md"]),
            added: owned(&["e.md"]),
            removed: owned(&["c.md"]),
        }
    );
    assert_eq!(second.hashed, 2, "only b.md + e.md read");

    let third = run(
        Some(&second.next),
        FP,
        scan_of(d, &["a.md", "b.md", "sub/d.md", "e.md"], false),
    );
    assert_eq!(third.freshness, Freshness::Fresh);
    assert_eq!(third.hashed, 0);
}

#[test]
fn touch_with_same_bytes_is_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    write(d, "a.md", b"same", Some(old_time()));
    let first = run(None, FP, scan_of(d, &["a.md"], false));
    set_time(&d.join("a.md"), old_time() + Duration::from_secs(60));
    let second = run(Some(&first.next), FP, scan_of(d, &["a.md"], false));
    assert_eq!(second.freshness, Freshness::Fresh);
    assert_eq!(second.hashed, 1);
    let entry = &second.next.files[&key_of("a.md")];
    assert_eq!(
        entry.mtime_ms,
        file_of(d, "a.md").modified_ms,
        "new mtime stored"
    );
    let third = run(Some(&second.next), FP, scan_of(d, &["a.md"], false));
    assert_eq!(third.hashed, 0);
}

#[test]
fn racy_entry_reported_changed_even_with_equal_stat() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    write(d, "fresh.md", b"one", None); // mtime = now -> racy
    write(d, "old.md", b"two", Some(old_time()));
    let first = run(None, FP, scan_of(d, &["fresh.md", "old.md"], false));
    assert!(first.next.files[&key_of("fresh.md")].racy);
    assert!(!first.next.files[&key_of("old.md")].racy);

    // same len + mtime (same-tick same-length edit) -> still changed
    let second = run(
        Some(&first.next),
        FP,
        scan_of(d, &["fresh.md", "old.md"], false),
    );
    assert_eq!(
        second.freshness,
        Freshness::Stale {
            changed: owned(&["fresh.md"]),
            added: Vec::new(),
            removed: Vec::new(),
        }
    );
    assert_eq!(second.hashed, 1);

    // once the mtime is old, entry verified -> fresh afterwards
    set_time(&d.join("fresh.md"), old_time());
    let third = run(
        Some(&second.next),
        FP,
        scan_of(d, &["fresh.md", "old.md"], false),
    );
    assert!(!third.next.files[&key_of("fresh.md")].racy);
    let fourth = run(
        Some(&third.next),
        FP,
        scan_of(d, &["fresh.md", "old.md"], false),
    );
    assert_eq!(fourth.freshness, Freshness::Fresh);
}

#[test]
fn none_or_other_fingerprint_adds_everything() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    write(d, "a.md", b"a", Some(old_time()));
    write(d, "b.md", b"b", Some(old_time()));
    let all = Freshness::Stale {
        changed: Vec::new(),
        added: owned(&["a.md", "b.md"]),
        removed: Vec::new(),
    };
    let first = run(None, FP, scan_of(d, &["b.md", "a.md"], false));
    assert_eq!(first.freshness, all);
    assert!(first.rebuild);
    assert_eq!(first.hashed, 2);
    assert_eq!(first.next.fingerprint, FP);

    let same = run(Some(&first.next), FP, scan_of(d, &["a.md", "b.md"], false));
    assert!(!same.rebuild);

    let other = run(
        Some(&first.next),
        "model-b/1/768/",
        scan_of(d, &["a.md", "b.md"], false),
    );
    assert_eq!(other.freshness, all);
    assert!(other.rebuild);
    assert_eq!(other.next.fingerprint, "model-b/1/768/");
}

#[test]
fn cut_off_scan_is_partial_and_keeps_unseen_entries() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    for p in ["a.md", "b.md", "c.md"] {
        write(d, p, p.as_bytes(), Some(old_time()));
    }
    let first = run(None, FP, scan_of(d, &["a.md", "b.md", "c.md"], false));
    write(d, "a.md", b"changed a", Some(old_time()));
    write(d, "n.md", b"n", Some(old_time()));
    let partial = run(Some(&first.next), FP, scan_of(d, &["a.md", "n.md"], true));
    assert_eq!(
        partial.freshness,
        Freshness::Partial {
            changed: owned(&["a.md"]),
            added: owned(&["n.md"]),
        }
    );
    assert_eq!(
        partial.next.files[&key_of("c.md")],
        first.next.files[&key_of("c.md")]
    );
    assert_eq!(partial.next.files.len(), 4);

    let unchanged = run(Some(&partial.next), FP, scan_of(d, &["a.md"], true));
    assert_eq!(
        unchanged.freshness,
        Freshness::Partial {
            changed: Vec::new(),
            added: Vec::new(),
        },
        "never fresh when cut off"
    );
}

#[test]
fn large_file_compared_by_stat_without_reading() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    write(d, "big.bin", b"small on disk", Some(old_time()));
    let big = |modified_ms| Scan {
        files: vec![ScanFile {
            size: HASH_LIMIT + 1,
            modified_ms,
            ..file_of(d, "big.bin")
        }],
        ..Scan::default()
    };
    let ms = file_of(d, "big.bin").modified_ms;
    let first = run(None, FP, big(ms));
    assert_eq!(first.hashed, 0);
    assert_eq!(first.next.files[&key_of("big.bin")].hash, None);
    assert_eq!(
        run(Some(&first.next), FP, big(ms)).freshness,
        Freshness::Fresh
    );
    let moved = run(Some(&first.next), FP, big(ms + 1000));
    assert_eq!(moved.hashed, 0);
    assert_eq!(
        moved.freshness,
        Freshness::Stale {
            changed: owned(&["big.bin"]),
            added: Vec::new(),
            removed: Vec::new(),
        }
    );
}

#[test]
fn unreadable_candidate_is_changed_without_hash() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    write(d, "a.md", b"a", Some(old_time()));
    let first = run(None, FP, scan_of(d, &["a.md"], false));
    let gone = Scan {
        files: vec![ScanFile {
            abs: Some(d.join("missing.md")),
            size: 2,
            ..file_of(d, "a.md")
        }],
        ..Scan::default()
    };
    let second = run(Some(&first.next), FP, gone);
    assert_eq!(second.hashed, 0);
    assert_eq!(second.next.files[&key_of("a.md")].hash, None);
    assert!(
        matches!(second.freshness, Freshness::Stale { ref changed, .. } if *changed == owned(&["a.md"]))
    );
}

#[test]
fn case_only_rename_is_unchanged_and_path_updated() {
    if !crate::paths::CASE_INSENSITIVE {
        return; // case-sensitive file systems: different case = different file
    }
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    write(d, "Note.md", b"n", Some(old_time()));
    let first = run(None, FP, scan_of(d, &["Note.md"], false));
    fs::rename(d.join("Note.md"), d.join("NOTE.md")).unwrap();
    let second = run(Some(&first.next), FP, scan_of(d, &["NOTE.md"], false));
    assert_eq!(second.freshness, Freshness::Fresh);
    assert_eq!(second.hashed, 0);
    assert_eq!(second.next.files[&key_of("Note.md")].path, "NOTE.md");
}

#[test]
fn nfd_spelling_has_same_key() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let nfc = "caf\u{e9}.md";
    let nfd = "cafe\u{301}.md";
    write(d, nfc, b"c", Some(old_time()));
    let first = run(None, FP, scan_of(d, &[nfc], false));
    let file = ScanFile {
        path: nfd.to_owned(),
        ..file_of(d, nfc)
    };
    let scan = Scan {
        files: vec![file],
        ..Scan::default()
    };
    let second = run(Some(&first.next), FP, scan);
    assert_eq!(second.freshness, Freshness::Fresh);
    assert_eq!(second.next.files[&key_of(nfc)].path, nfd);
}

#[test]
fn ten_thousand_unchanged_files_hash_nothing() {
    let modified_ms =
        i64::try_from(old_time().duration_since(UNIX_EPOCH).unwrap().as_millis()).unwrap();
    let files: Vec<ScanFile> = (0..10_000)
        .map(|i| ScanFile {
            path: format!("dir{}/file{i}.md", i % 100),
            category: Category::Markdown,
            abs: Some(PathBuf::from(format!("Z:/nowhere/file{i}.md"))),
            size: 100,
            modified_ms,
        })
        .collect();
    let prev = Snapshot {
        fingerprint: FP.to_owned(),
        files: files
            .iter()
            .map(|f| {
                let entry = Entry {
                    path: f.path.clone(),
                    len: f.size,
                    mtime_ms: f.modified_ms,
                    hash: Some(1),
                    racy: false,
                };
                (key_of(&f.path), entry)
            })
            .collect(),
    };
    let scan = Scan {
        files,
        ..Scan::default()
    };
    let checked = run(Some(&prev), FP, scan);
    assert_eq!(checked.hashed, 0);
    assert_eq!(checked.freshness, Freshness::Fresh);
    assert_eq!(checked.next, prev);
}

#[test]
fn snapshot_survives_serde_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    write(d, "a.md", b"a", Some(old_time()));
    write(d, "big.bin", b"b", Some(old_time()));
    let first = run(None, FP, scan_of(d, &["a.md", "big.bin"], false));
    let text = serde_json::to_string(&first.next).unwrap();
    let back: Snapshot = serde_json::from_str(&text).unwrap();
    assert_eq!(back, first.next);
    let second = run(Some(&back), FP, scan_of(d, &["a.md", "big.bin"], false));
    assert_eq!(second.freshness, Freshness::Fresh);
    assert_eq!(second.hashed, 0);
}

#[test]
fn scan_error_passes_through() {
    let result = check(None, FP, || Err::<Scan, _>("walk failed"));
    assert_eq!(result.err(), Some("walk failed"));
}
