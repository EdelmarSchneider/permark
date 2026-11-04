//! The plugin's own folders in a store: `<store>/<lineage>/<version>/` (decimal version) of lineages it knows. ONLY
//! these are ever removed | counted: a chosen store is the person's folder, anything else in it stays theirs. A link
//! in the place of a version folder is removed as a link, never followed.

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};

use permark_core::fsx;

/// Folder of one installed version (layout of [`crate::catalog::install_folder`]).
pub(crate) fn version_dir(store: &Path, lineage: &str, catalog_version: u32) -> PathBuf {
    store.join(lineage).join(catalog_version.to_string())
}

/// Removes the version folder of `lineage` + the lineage folder once empty (`remove_dir`: only empty ones; never a
/// link followed).
pub(crate) fn prune(store: &Path, lineage: &str, catalog_version: u32) {
    let _ = std::fs::remove_dir(version_dir(store, lineage, catalog_version));
    let _ = std::fs::remove_dir(store.join(lineage));
}

/// One version folder found: lineage, version, path, whether it is a real folder (else a link | file).
struct Found {
    lineage: String,
    version: u32,
    path: PathBuf,
    folder: bool,
}

/// Version folders of `lineages` in `store` (names that read back as the same decimal number only).
fn versions(store: &Path, lineages: &BTreeSet<String>) -> Vec<Found> {
    let mut found = Vec::new();
    for lineage in lineages {
        let dir = store.join(lineage);
        // a lineage folder that is a link: not ours to walk
        if !std::fs::symlink_metadata(&dir).is_ok_and(|m| m.is_dir()) {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(version) = name
                .to_str()
                .and_then(|n| n.parse::<u32>().ok().filter(|v| v.to_string() == n))
            else {
                continue;
            };
            let path = entry.path();
            let folder = std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_dir());
            found.push(Found {
                lineage: lineage.clone(),
                version,
                path,
                folder,
            });
        }
    }
    found
}

/// Removes one version folder (Windows locks retried); a link | file in its place: removed itself.
fn remove_version(found: &Found) -> io::Result<()> {
    fsx::retry(&fsx::RETRY_DELAYS, || {
        if found.folder {
            std::fs::remove_dir_all(&found.path)
        } else {
            std::fs::remove_file(&found.path).or_else(|_| std::fs::remove_dir(&found.path))
        }
    })
}

/// Bytes of the files in the version folders of `lineages` (installed files + parts; links not followed).
pub(crate) fn bytes(store: &Path, lineages: &BTreeSet<String>) -> u64 {
    versions(store, lineages)
        .iter()
        .filter(|f| f.folder)
        .filter_map(|f| std::fs::read_dir(&f.path).ok())
        .flat_map(Iterator::flatten)
        .filter_map(|e| std::fs::symlink_metadata(e.path()).ok())
        .filter(std::fs::Metadata::is_file)
        .map(|m| m.len())
        .sum()
}

/// "Remove data" of a chosen store: every version folder of `lineages`, then lineage folders + the store once empty.
///
/// # Errors
/// First removal that failed (others still tried).
pub(crate) fn remove_all(store: &Path, lineages: &BTreeSet<String>) -> io::Result<()> {
    let mut first = None;
    for found in versions(store, lineages) {
        if let Err(error) = remove_version(&found) {
            first.get_or_insert(error);
        }
    }
    for lineage in lineages {
        let _ = std::fs::remove_dir(store.join(lineage));
    }
    let _ = std::fs::remove_dir(store);
    first.map_or(Ok(()), Err)
}

/// Startup of a store: removes the version folders of `lineages` that `keep` does not name (left by a lost state file
/// | an old version whose record is gone). Best effort.
pub(crate) fn remove_orphans(
    store: &Path,
    lineages: &BTreeSet<String>,
    keep: impl Fn(&str, u32) -> bool,
) {
    for found in versions(store, lineages) {
        if !keep(&found.lineage, found.version) && remove_version(&found).is_ok() {
            let _ = std::fs::remove_dir(store.join(&found.lineage));
        }
    }
}
