//! The plugin's own folders in a store: `<store>/<lineage>/<version>/` (decimal version) of lineages it knows. ONLY
//! these are ever removed | counted: a chosen store is the person's folder, anything else in it stays theirs. A link
//! (symlink | junction) in the place of a lineage | version folder is skipped, never followed nor removed; the check
//! is made again right before each removal.

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};

use permark_core::downloads::MANIFEST_SUFFIX;
use permark_core::fsx;

/// Name ending of the copies a store move writes before checking them ([`scratch_name`]).
const SCRATCH_SUFFIX: &str = ".move.part";

/// Folder of one installed version (layout of [`crate::catalog::install_folder`]).
pub(crate) fn version_dir(store: &Path, lineage: &str, catalog_version: u32) -> PathBuf {
    store.join(lineage).join(catalog_version.to_string())
}

/// Hidden name a store move copies asset `name` into (`.<name>.move.part`, a scratch name for
/// [`fsx::is_scratch_name`]: never listed, never swept by the download engine).
pub(crate) fn scratch_name(name: &str) -> String {
    format!(".{name}{SCRATCH_SUFFIX}")
}

fn is_move_scratch(name: &str) -> bool {
    name.starts_with('.') && name.ends_with(SCRATCH_SUFFIX) && fsx::is_scratch_name(name)
}

/// Removes the version folder of `lineage` + the lineage folder once empty (`remove_dir`: only empty ones; never a
/// link followed).
pub(crate) fn prune(store: &Path, lineage: &str, catalog_version: u32) {
    let _ = std::fs::remove_dir(version_dir(store, lineage, catalog_version));
    let _ = std::fs::remove_dir(store.join(lineage));
}

/// One real version folder found.
struct Found {
    lineage: String,
    version: u32,
    path: PathBuf,
}

/// A real folder (not a link | junction | file).
fn real_dir(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir())
}

/// Real version folders of `lineages` in `store` (names that read back as the same decimal number only).
fn versions(store: &Path, lineages: &BTreeSet<String>) -> Vec<Found> {
    let mut found = Vec::new();
    for lineage in lineages {
        let dir = store.join(lineage);
        // a lineage folder that is a link: not ours to walk
        if !real_dir(&dir) {
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
            if real_dir(&path) {
                found.push(Found {
                    lineage: lineage.clone(),
                    version,
                    path,
                });
            }
        }
    }
    found
}

/// Removes one version folder (Windows locks retried). Lineage | version folder turned into a link since it was
/// found: skipped.
fn remove_version(store: &Path, found: &Found) -> io::Result<()> {
    if !real_dir(&store.join(&found.lineage)) || !real_dir(&found.path) {
        return Ok(());
    }
    fsx::retry(&fsx::RETRY_DELAYS, || std::fs::remove_dir_all(&found.path))
}

/// Names of the entries of `dir` (unreadable: none).
fn names(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// `dir` holds an engine install (manifest) | a download part (`.part`, `.part.meta`; not a scratch name): another
/// profile | app version may own it. Unreadable: held (never removed).
fn held(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return true;
    };
    entries.flatten().any(|e| {
        let name = e.file_name().to_string_lossy().into_owned();
        // `.part` | `.part.meta` (any case; a stray `.meta` counts too: kept rather than removed)
        let part = Path::new(&name)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("part") || e.eq_ignore_ascii_case("meta"));
        name.ends_with(MANIFEST_SUFFIX) || (part && !fsx::is_scratch_name(&name))
    })
}

/// Bytes of the files in the version folders of `lineages` (installed files + parts; links not followed).
pub(crate) fn bytes(store: &Path, lineages: &BTreeSet<String>) -> u64 {
    versions(store, lineages)
        .iter()
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
        if let Err(error) = remove_version(store, &found) {
            first.get_or_insert(error);
        }
    }
    for lineage in lineages {
        let _ = std::fs::remove_dir(store.join(lineage));
    }
    let _ = std::fs::remove_dir(store);
    first.map_or(Ok(()), Err)
}

/// Tidy of a store (caller holds the store to itself: `Downloads::while_idle`): removes the version folders of
/// `lineages` that `keep` does not name AND that hold no install | part (left by a lost state file | an old version
/// whose record is gone | a move that stopped), then the move copies left in any version folder (a crash mid-move).
/// Best effort.
pub(crate) fn tidy(store: &Path, lineages: &BTreeSet<String>, keep: impl Fn(&str, u32) -> bool) {
    for found in versions(store, lineages) {
        if !keep(&found.lineage, found.version) && !held(&found.path) {
            if remove_version(store, &found).is_ok() {
                let _ = std::fs::remove_dir(store.join(&found.lineage));
            }
            continue;
        }
        for name in names(&found.path).iter().filter(|n| is_move_scratch(n)) {
            let _ = std::fs::remove_file(found.path.join(name));
        }
    }
}
