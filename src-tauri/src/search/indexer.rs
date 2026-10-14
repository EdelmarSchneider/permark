//! Building the search index from the vault, and keeping it current from watcher output.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;

use super::chunk::chunk_markdown;
use super::Bm25Index;
use crate::render::{decode_text, MAX_RENDER_BYTES};
use crate::util::{hash_bytes, modified_ms, read_with_retry};
use crate::vault::{NodeKind, Vault};
use crate::watch::{ChangeKind, FileSig, FsChange, PathFilter, SigStore, Verdict};

/// How often `build` reports progress (in files).
const PROGRESS_EVERY: usize = 25;

/// Only Markdown is indexed.
pub fn is_indexable(rel: &str) -> bool {
    Path::new(rel).extension().is_some_and(|e| {
        let e = e.to_string_lossy().to_ascii_lowercase();
        matches!(e.as_str(), "md" | "markdown" | "mdown" | "mkd")
    })
}

/// Reads one file, remembers its signature, and (re)indexes it. Returns whether it was indexed.
///
/// Files that cannot be read, are too large or binary are removed from the index instead.
pub fn index_file(abs: &Path, rel: &str, index: &RwLock<Bm25Index>, sigs: &SigStore) -> bool {
    let chunks = (|| {
        let meta = std::fs::metadata(abs).ok()?;
        if !meta.is_file() || meta.len() > MAX_RENDER_BYTES {
            return None;
        }
        let bytes = read_with_retry(abs).ok()?;
        sigs.set(
            crate::vault::key(rel),
            rel,
            FileSig {
                len: bytes.len() as u64,
                mtime_ms: modified_ms(&meta),
                hash: hash_bytes(&bytes),
            },
        );
        Some(chunk_markdown(&decode_text(&bytes)?))
    })();
    let Ok(mut guard) = index.write() else {
        return false;
    };
    if let Some(chunks) = chunks {
        guard.add_chunks(rel, chunks);
        true
    } else {
        guard.remove_doc(rel);
        false
    }
}

/// Indexes every Markdown file of the vault. `progress(indexed, total)` is called every few files and
/// at the end; the loop stops early when `cancel` is set (a different folder was opened).
pub fn build(
    vault: &Vault,
    filter: &PathFilter,
    index: &RwLock<Bm25Index>,
    sigs: &SigStore,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(usize, usize),
) {
    let files: Vec<_> = vault
        .walk()
        .into_iter()
        .filter(|item| item.kind == NodeKind::File && is_indexable(&item.rel))
        .filter(|item| {
            vault
                .locate(&item.abs)
                .is_some_and(|l| filter.classify(l.root, &l.inner, false) == Verdict::Emit)
        })
        .collect();
    let total = files.len();
    progress(0, total);
    for (i, item) in files.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        index_file(&item.abs, &item.rel, index, sigs);
        let done = i + 1;
        if done % PROGRESS_EVERY == 0 || done == total {
            progress(done, total);
        }
    }
}

/// Applies watcher output to the index.
pub fn apply_changes(
    vault: &Vault,
    index: &RwLock<Bm25Index>,
    sigs: &SigStore,
    changes: &[FsChange],
) {
    for change in changes {
        match change.kind {
            ChangeKind::Removed => remove(index, &change.path),
            ChangeKind::Renamed | ChangeKind::Created | ChangeKind::Modified => {
                if let Some(from) = &change.from {
                    remove(index, from);
                }
                if !is_indexable(&change.path) {
                    continue;
                }
                match vault.resolve(&change.path) {
                    Ok(r) if r.abs.is_file() => {
                        index_file(&r.abs, &r.rel, index, sigs);
                    }
                    _ => remove(index, &change.path),
                }
            }
        }
    }
}

fn remove(index: &RwLock<Bm25Index>, rel: &str) {
    if let Ok(mut guard) = index.write() {
        guard.remove_doc(rel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::SearchProvider;
    use std::fs;

    struct Fx {
        dir: tempfile::TempDir,
        vault: Vault,
        filter: PathFilter,
        index: RwLock<Bm25Index>,
        sigs: SigStore,
    }

    fn fx(files: &[(&str, &str)]) -> Fx {
        let dir = tempfile::tempdir().expect("tempdir");
        for (rel, content) in files {
            let p = dir.path().join(rel);
            fs::create_dir_all(p.parent().expect("parent")).expect("mkdir");
            fs::write(p, content).expect("write");
        }
        let vault = Vault::open(dir.path()).expect("open");
        let filter = PathFilter::new(
            &[(vault.root().to_path_buf(), None)],
            &["private/".to_owned()],
        );
        Fx {
            dir,
            vault,
            filter,
            index: RwLock::new(Bm25Index::new()),
            sigs: SigStore::new(),
        }
    }

    fn build_all(f: &Fx) -> Vec<(usize, usize)> {
        let mut calls = Vec::new();
        build(
            &f.vault,
            &f.filter,
            &f.index,
            &f.sigs,
            &AtomicBool::new(false),
            &mut |a, b| calls.push((a, b)),
        );
        calls
    }

    #[test]
    fn build_indexes_markdown_only_honoring_ignores_and_reports_progress() {
        let f = fx(&[
            ("a.md", "# Alpha\n\nuniquealphaword\n"),
            ("docs/b.markdown", "# Beta\n\nbeta text\n"),
            ("notes.txt", "# Not markdown\n\nplainonlyword\n"),
            ("private/c.md", "# Hidden\n\nhiddenword\n"),
            ("node_modules/pkg/readme.md", "# Dep\n\ndepword\n"),
            (".gitignore", "ignored.md\n"),
            ("ignored.md", "# Ign\n\nignoredword\n"),
        ]);
        let calls = build_all(&f);
        assert_eq!(calls.first(), Some(&(0, 2)));
        assert_eq!(calls.last(), Some(&(2, 2)));
        let i = f.index.read().expect("lock");
        assert_eq!(i.paths(), ["a.md", "docs/b.markdown"]);
        assert!(i.search("plainonlyword", 3).is_empty());
        assert!(i.search("hiddenword", 3).is_empty());
        assert!(i.search("ignoredword", 3).is_empty());
        assert_eq!(i.search("uniquealphaword", 3).len(), 1);
        drop(i);
        assert_eq!(f.sigs.len(), 2, "signatures are seeded for the watcher");
    }

    #[test]
    fn cancel_stops_the_build() {
        let f = fx(&[("a.md", "# A\n\nx\n"), ("b.md", "# B\n\ny\n")]);
        let cancel = AtomicBool::new(true);
        build(
            &f.vault,
            &f.filter,
            &f.index,
            &f.sigs,
            &cancel,
            &mut |_, _| {},
        );
        assert!(f.index.read().expect("lock").is_empty());
    }

    #[test]
    fn changes_update_the_index_incrementally() {
        let f = fx(&[("a.md", "# A\n\nfirstversion\n")]);
        build_all(&f);
        let search = |q: &str| f.index.search(q, 3);
        assert_eq!(search("firstversion").len(), 1);

        fs::write(f.dir.path().join("a.md"), "# A\n\nsecondversion\n").expect("write");
        fs::write(f.dir.path().join("new.md"), "# New\n\nbrandnewword\n").expect("write");
        let ch = |kind, path: &str| FsChange {
            kind,
            path: path.to_owned(),
            from: None,
            at_ms: 0,
        };
        apply_changes(
            &f.vault,
            &f.index,
            &f.sigs,
            &[
                ch(ChangeKind::Modified, "a.md"),
                ch(ChangeKind::Created, "new.md"),
            ],
        );
        assert!(search("firstversion").is_empty());
        assert_eq!(search("secondversion").len(), 1);
        assert_eq!(search("brandnewword").len(), 1);

        fs::remove_file(f.dir.path().join("new.md")).expect("rm");
        apply_changes(
            &f.vault,
            &f.index,
            &f.sigs,
            &[ch(ChangeKind::Removed, "new.md")],
        );
        assert!(search("brandnewword").is_empty());
    }

    #[test]
    fn renames_move_the_document_in_the_index() {
        let f = fx(&[("old.md", "# T\n\nrenamemeword\n")]);
        build_all(&f);
        fs::create_dir_all(f.dir.path().join("sub")).expect("mkdir");
        fs::rename(f.dir.path().join("old.md"), f.dir.path().join("sub/new.md")).expect("mv");
        apply_changes(
            &f.vault,
            &f.index,
            &f.sigs,
            &[FsChange {
                kind: ChangeKind::Renamed,
                path: "sub/new.md".to_owned(),
                from: Some("old.md".to_owned()),
                at_ms: 0,
            }],
        );
        let hits = f.index.search("renamemeword", 3);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "sub/new.md");
    }

    #[test]
    fn unreadable_or_binary_files_leave_the_index() {
        let f = fx(&[("a.md", "# A\n\nsomeword\n")]);
        build_all(&f);
        fs::write(f.dir.path().join("a.md"), b"\0\0binary\0").expect("write");
        assert!(!index_file(
            &f.dir.path().join("a.md"),
            "a.md",
            &f.index,
            &f.sigs
        ));
        assert!(f.index.search("someword", 3).is_empty());
        assert!(!index_file(
            &f.dir.path().join("missing.md"),
            "missing.md",
            &f.index,
            &f.sigs
        ));
    }

    #[test]
    fn indexable_extensions() {
        assert!(is_indexable("a/B.MD"));
        assert!(is_indexable("x.markdown"));
        assert!(!is_indexable("x.txt"));
        assert!(!is_indexable("md"));
    }
}
