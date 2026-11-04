//! Store move (D-8, AC-7): every installed version copied into the new store (same `<lineage>/<version>/` layout),
//! sha256-checked against its pins WHILE copying, engine manifest written LAST ([`Downloads::adopt`]). Any failure |
//! cancel: what THIS move made is removed ([`Made`]), nothing else; the old store untouched. Removing the old copies
//! is the caller's step once the new store is in use. Always a copy (also on one volume: the old files stay until the
//! new ones are verified).
//!
//! Target safety: a version the target already reports installed is skipped (never truncated, never removed on
//! failure); the unit folder is refused when a link | junction | file is on the way (checked before and after the
//! folders are made); each file is streamed (GB-sized: never held in memory) into a new scratch name
//! ([`crate::store::scratch_name`], opened `create_new`: never through an existing file | link) and renamed into place
//! once checked. Scratch files a crash left are removed by the next tidy of that store ([`crate::store::tidy`]).

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use permark_core::downloads::{DownloadError, Downloads, Unit, MANIFEST_SUFFIX};
use permark_core::fsx;
use sha2::{Digest, Sha256};

use crate::state::Installed;
use crate::store::scratch_name;

/// Copy chunk.
const CHUNK: usize = 1 << 20;
/// Least time between two progress reports.
const REPORT_EVERY: Duration = Duration::from_millis(100);

/// One installed version to move.
#[derive(Debug, Clone)]
pub(crate) struct Item {
    pub lineage: String,
    pub installed: Installed,
}

impl Item {
    fn unit(&self) -> Result<Unit, DownloadError> {
        self.installed
            .unit(&self.lineage)
            .map_err(|e| DownloadError::Disk {
                detail: e.to_string(),
            })
    }

    pub(crate) fn bytes(&self) -> u64 {
        self.installed.files.iter().map(|f| f.bytes).sum()
    }
}

/// What one move made in the target store: removed again by [`discard`] (nothing else is).
#[derive(Debug, Default)]
pub(crate) struct Made {
    /// Folders made (outermost first).
    dirs: Vec<PathBuf>,
    /// Files made (scratch copies | checked copies under their final names).
    files: Vec<PathBuf>,
    /// Manifests written (units adopted).
    manifests: Vec<PathBuf>,
}

/// Throttled progress over the whole move.
struct Meter<'a> {
    done: u64,
    total: u64,
    last: Option<Instant>,
    report: &'a mut dyn FnMut(u64, u64),
}

impl Meter<'_> {
    fn add(&mut self, bytes: u64) {
        self.done += bytes;
        if self.last.is_none_or(|t| t.elapsed() >= REPORT_EVERY) || self.done == self.total {
            self.last = Some(Instant::now());
            (self.report)(self.done, self.total);
        }
    }
}

fn disk(error: &io::Error) -> DownloadError {
    DownloadError::disk(error)
}

/// Streams `from` into the new scratch file `to` while hashing; size + sha256 must match the pins.
fn copy_checked(
    from: &Path,
    to: &Path,
    pins: (u64, &[u8; 32]),
    cancel: &AtomicBool,
    meter: &mut Meter<'_>,
    made: &mut Made,
) -> Result<(), DownloadError> {
    let (bytes, sha256) = pins;
    let mut source = File::open(from).map_err(|e| disk(&e))?;
    if source.metadata().map_err(|e| disk(&e))?.len() != bytes {
        return Err(DownloadError::Integrity);
    }
    // GB-sized stream into the plugin's own store under a new scratch name (never an existing file | link);
    // installed only by the manifest written last (core `adopt`)
    let mut target = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(to)
        .map_err(|e| disk(&e))?;
    made.files.push(to.to_path_buf());
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; CHUNK];
    let mut copied = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(DownloadError::Cancelled);
        }
        let read = source.read(&mut buffer).map_err(|e| disk(&e))?;
        if read == 0 {
            break;
        }
        let chunk = &buffer[..read];
        hasher.update(chunk);
        target.write_all(chunk).map_err(|e| disk(&e))?;
        copied += read as u64;
        meter.add(read as u64);
    }
    target.sync_all().map_err(|e| disk(&e))?;
    let digest: [u8; 32] = hasher.finalize().into();
    if copied != bytes || &digest != sha256 {
        return Err(DownloadError::Integrity);
    }
    Ok(())
}

/// Makes the folders of `unit` in `store` (each one made recorded); refused when a link | file is on the way, before
/// and after.
fn unit_dir(unit: &Unit, store: &Path, made: &mut Made) -> Result<PathBuf, DownloadError> {
    let dir = unit.dir_in(store).map_err(|e| disk(&e))?;
    let mut missing: Vec<PathBuf> = dir
        .ancestors()
        .take_while(|p| *p != store && std::fs::symlink_metadata(p).is_err())
        .map(Path::to_path_buf)
        .collect();
    missing.reverse();
    std::fs::create_dir_all(&dir).map_err(|e| disk(&e))?;
    made.dirs.extend(missing);
    // nothing swapped in for a folder meanwhile
    unit.dir_in(store).map_err(|e| disk(&e))
}

/// Copies one version + writes its manifest in `target`'s store. Already installed there: skipped.
fn copy_item(
    from: &Path,
    item: &Item,
    target: &Downloads,
    cancel: &AtomicBool,
    meter: &mut Meter<'_>,
    made: &mut Made,
) -> Result<(), DownloadError> {
    let unit = item.unit()?;
    if target.is_installed(&unit) {
        meter.add(item.bytes());
        return Ok(());
    }
    let source = unit.dir_in(from).map_err(|e| disk(&e))?;
    let dir = unit_dir(&unit, target.store(), made)?;
    for asset in unit.assets() {
        let scratch = dir.join(scratch_name(asset.name()));
        copy_checked(
            &source.join(asset.name()),
            &scratch,
            (asset.bytes(), asset.sha256()),
            cancel,
            meter,
            made,
        )?;
        let place = dir.join(asset.name());
        // a file there (a download's finished file of the same pins) is replaced by the checked copy; a link |
        // folder: refused
        let there = match std::fs::symlink_metadata(&place) {
            Ok(meta) if meta.is_file() => true,
            Ok(_) => {
                return Err(DownloadError::Disk {
                    detail: "model file place is a link or a folder".to_owned(),
                })
            }
            Err(_) => false,
        };
        fsx::retry(&fsx::RETRY_DELAYS, || std::fs::rename(&scratch, &place))
            .map_err(|e| disk(&e))?;
        made.files.retain(|f| f != &scratch);
        if !there {
            made.files.push(place);
        }
    }
    target.adopt(&unit)?;
    made.manifests
        .push(dir.join(format!("{}{MANIFEST_SUFFIX}", unit.id())));
    Ok(())
}

/// Removes what one move made in the target: manifests first (no longer installed), files, then its folders once
/// empty. Anything the target held before stays.
pub(crate) fn discard(made: &Made) {
    for path in made.manifests.iter().chain(&made.files) {
        let _ = fsx::retry(&fsx::RETRY_DELAYS, || match std::fs::remove_file(path) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            other => other,
        });
    }
    for dir in made.dirs.iter().rev() {
        let _ = std::fs::remove_dir(dir);
    }
}

/// Copies every item from store `from` into `target`'s store; what it made (for [`discard`] when the switch fails).
/// Failure | cancel: what it made removed.
///
/// # Errors
/// `Cancelled`, `Integrity` (a source file of another size | content than its pins), `Disk` (incl. a link on the
/// way in the target), `Busy`.
pub(crate) fn copy_all(
    from: &Path,
    items: &[Item],
    target: &Downloads,
    cancel: &AtomicBool,
    report: &mut dyn FnMut(u64, u64),
) -> Result<Made, DownloadError> {
    let mut meter = Meter {
        done: 0,
        total: items.iter().map(Item::bytes).sum(),
        last: None,
        report,
    };
    let mut made = Made::default();
    let result = items
        .iter()
        .try_for_each(|item| copy_item(from, item, target, cancel, &mut meter, &mut made));
    match result {
        Ok(()) => Ok(made),
        Err(error) => {
            discard(&made);
            Err(error)
        }
    }
}
