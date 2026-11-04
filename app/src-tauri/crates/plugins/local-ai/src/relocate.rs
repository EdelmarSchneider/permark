//! Store move (D-8, AC-7): every installed version copied into the new store (same `<lineage>/<version>/` layout),
//! sha256-checked against its pins WHILE copying, engine manifest written LAST ([`Downloads::adopt`]). Any failure |
//! cancel: what was copied is removed, the old store untouched. Removing the old copies is the caller's step once
//! the new store is in use. Always a copy (also on one volume: the old files stay until the new ones are verified).
//!
//! Files are streamed into their final names (GB-sized: never held in memory); a version is installed only once its
//! manifest exists, so a half copy never reads as installed.

use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use permark_core::downloads::{DownloadError, Downloads, Unit};
use sha2::{Digest, Sha256};

use crate::state::Installed;
use crate::store::{prune, version_dir};

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

/// Streams `from` into a new `to` while hashing; size + sha256 must match the pins.
fn copy_checked(
    from: &Path,
    to: &Path,
    bytes: u64,
    sha256: &[u8; 32],
    cancel: &AtomicBool,
    meter: &mut Meter<'_>,
) -> Result<(), DownloadError> {
    let mut source = File::open(from).map_err(|e| disk(&e))?;
    if source.metadata().map_err(|e| disk(&e))?.len() != bytes {
        return Err(DownloadError::Integrity);
    }
    // GB-sized stream into the plugin's own store; installed only by the manifest written last (core `adopt`)
    let mut target = File::create(to).map_err(|e| disk(&e))?; // arch-allow: ARCH-002 streamed model copy, not a user file
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

/// Copies one version + writes its manifest in `target`'s store.
fn copy_item(
    from: &Path,
    item: &Item,
    target: &Downloads,
    cancel: &AtomicBool,
    meter: &mut Meter<'_>,
) -> Result<(), DownloadError> {
    let unit = item.unit()?;
    let version = item.installed.catalog_version;
    let source = version_dir(from, &item.lineage, version);
    let dir = version_dir(target.store(), &item.lineage, version);
    std::fs::create_dir_all(&dir).map_err(|e| disk(&e))?;
    for asset in unit.assets() {
        copy_checked(
            &source.join(asset.name()),
            &dir.join(asset.name()),
            asset.bytes(),
            asset.sha256(),
            cancel,
            meter,
        )?;
    }
    target.adopt(&unit)
}

/// Removes the copies of `items` from `target`'s store (manifest first; folders once empty).
pub(crate) fn discard(items: &[Item], target: &Downloads) {
    for item in items {
        if let Ok(unit) = item.unit() {
            let _ = target.remove(&unit);
        }
        prune(
            target.store(),
            &item.lineage,
            item.installed.catalog_version,
        );
    }
}

/// Copies every item from store `from` into `target`'s store. Failure | cancel: the copies removed.
///
/// # Errors
/// `Cancelled`, `Integrity` (a source file of another size | content than its pins), `Disk`, `Busy`.
pub(crate) fn copy_all(
    from: &Path,
    items: &[Item],
    target: &Downloads,
    cancel: &AtomicBool,
    report: &mut dyn FnMut(u64, u64),
) -> Result<(), DownloadError> {
    let mut meter = Meter {
        done: 0,
        total: items.iter().map(Item::bytes).sum(),
        last: None,
        report,
    };
    let result = items
        .iter()
        .try_for_each(|item| copy_item(from, item, target, cancel, &mut meter));
    if result.is_err() {
        discard(items, target);
    }
    result
}
